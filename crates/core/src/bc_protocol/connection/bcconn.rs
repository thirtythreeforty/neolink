use super::BcSubscription;
use crate::{bc::model::*, Error, Result};
use futures::future::BoxFuture;
use futures::sink::{Sink, SinkExt};
use futures::stream::{Stream, StreamExt};
use log::*;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Sender};
use tokio::task::yield_now;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::PollSender;

use tokio::{sync::RwLock, task::JoinSet};

type MsgHandler = dyn 'static + Send + Sync + for<'a> Fn(&'a Bc) -> BoxFuture<'a, Option<Bc>>;

#[derive(Default)]
struct Subscriber {
    /// Subscribers based on their ID and their num
    num: BTreeMap<u32, BTreeMap<Option<u16>, Sender<Result<Bc>>>>,
    /// Subscribers based on their ID
    id: BTreeMap<u32, Arc<MsgHandler>>,
}

pub(crate) type BcConnSink = Box<dyn Sink<Bc, Error = Error> + Send + Sync + Unpin>;
pub(crate) type BcConnSource = Box<dyn Stream<Item = Result<Bc>> + Send + Sync + Unpin>;

/// A shareable connection to a camera.  Handles serialization of messages.  To send/receive, call
/// .[subscribe()] with a message number.  You can use the BcSubscription to send or receive only
/// messages with that number; each incoming message is routed to its appropriate subscriber.
///
/// There can be only one subscriber per kind of message at a time.
pub struct BcConnection {
    sink: Sender<Result<Bc>>,
    poll_commander: Sender<PollCommand>,
    rx_thread: RwLock<JoinSet<Result<()>>>,
}

impl BcConnection {
    pub async fn new(mut sink: BcConnSink, source: BcConnSource) -> Result<BcConnection> {
        let (sinker, sinker_rx) = channel::<Result<Bc>>(100);

        let (poll_commander, poll_commanded) = channel(200);
        let mut poller = Poller {
            subscribers: Default::default(),
            sink: sinker.clone(),
            reciever: ReceiverStream::new(poll_commanded),
        };

        let mut rx_thread = JoinSet::<Result<()>>::new();
        let thread_poll_commander = poll_commander.clone();
        let handle = tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            runtime.block_on(
                PollSender::new(thread_poll_commander)
                    .send_all(&mut source.map(|bc| Ok(PollCommand::Bc(Box::new(bc))))),
            )
        });
        rx_thread.spawn(async move {
            handle.await??;
            Ok(())
        });

        rx_thread.spawn(async move { sink.send_all(&mut ReceiverStream::new(sinker_rx)).await });

        rx_thread.spawn(async move {
            loop {
                poller.run().await?;
            }
        });

        Ok(BcConnection {
            sink: sinker,
            poll_commander,
            rx_thread: RwLock::new(rx_thread),
        })
    }

    pub(super) async fn send(&self, bc: Bc) -> crate::Result<()> {
        self.sink.send(Ok(bc)).await?;
        Ok(())
    }

    pub async fn subscribe(&self, msg_id: u32, msg_num: u16) -> Result<BcSubscription> {
        let (tx, rx) = channel(100);
        self.poll_commander
            .send(PollCommand::AddSubscriber(msg_id, Some(msg_num), tx))
            .await?;
        Ok(BcSubscription::new(rx, Some(msg_num as u32), self))
    }

    /// Some messages are initiated by the camera. This creates a handler for them
    /// It requires a closure that will be used to handle the message
    /// and return either None or Some(Bc) reply
    pub async fn handle_msg<T>(&self, msg_id: u32, handler: T) -> Result<()>
    where
        T: 'static + Send + Sync + for<'a> Fn(&'a Bc) -> BoxFuture<'a, Option<Bc>>,
    {
        self.poll_commander
            .send(PollCommand::AddHandler(msg_id, Arc::new(handler)))
            .await?;
        Ok(())
    }

    /// Stop a message handler created using [`handle_msg`]
    #[allow(dead_code)] // Currently unused but added for future use
    pub async fn unhandle_msg(&self, msg_id: u32) -> Result<()> {
        self.poll_commander
            .send(PollCommand::RemoveHandler(msg_id))
            .await?;
        Ok(())
    }

    /// Some times we want to wait for a reply on a new message ID
    /// to do this we wait for the next packet with a certain ID
    /// grab it's message ID and then subscribe to that ID
    ///
    /// The command Snap that grabs a jpeg payload is an example of this
    ///
    /// This function creates a temporary handle to grab this single message
    pub async fn subscribe_to_id(&self, msg_id: u32) -> Result<BcSubscription> {
        let (tx, rx) = channel(100);
        self.poll_commander
            .send(PollCommand::AddSubscriber(msg_id, None, tx))
            .await?;
        Ok(BcSubscription::new(rx, None, self))
    }

    pub(crate) async fn join(&self) -> Result<()> {
        let mut locked_threads = self.rx_thread.write().await;
        while let Some(res) = locked_threads.join_next().await {
            match res {
                Err(e) => {
                    locked_threads.abort_all();
                    return Err(e.into());
                }
                Ok(Err(e)) => {
                    locked_threads.abort_all();
                    return Err(e);
                }
                Ok(Ok(())) => {}
            }
        }
        Ok(())
    }
}

enum PollCommand {
    Bc(Box<Result<Bc>>),
    AddHandler(u32, Arc<MsgHandler>),
    RemoveHandler(u32),
    AddSubscriber(u32, Option<u16>, Sender<Result<Bc>>),
}

struct Poller {
    subscribers: Subscriber,
    sink: Sender<Result<Bc>>,
    reciever: ReceiverStream<PollCommand>,
}

impl Poller {
    async fn run(&mut self) -> Result<()> {
        while let Some(command) = self.reciever.next().await {
            yield_now().await;
            match command {
                PollCommand::Bc(boxed_response) => {
                    match *boxed_response {
                        Ok(response) => {
                            let msg_num = response.meta.msg_num;
                            let msg_id = response.meta.msg_id;

                            match (
                                self.subscribers.id.get(&msg_id),
                                self.subscribers.num.get_mut(&msg_id),
                            ) {
                                (Some(occ), _) => {
                                    if let Some(reply) = occ(&response).await {
                                        assert!(reply.meta.msg_num == response.meta.msg_num);
                                        self.sink.send(Ok(reply)).await?;
                                    }
                                }
                                (None, Some(occ)) => {
                                    let sender = if let Some(sender) =
                                        occ.get(&Some(msg_num)).filter(|a| !a.is_closed()).cloned()
                                    {
                                        // Connection with id exists and is not closed
                                        Some(sender)
                                    } else if let Some(sender) = occ.get(&None).cloned() {
                                        // Upgrade a None to a known MsgID
                                        occ.remove(&None);
                                        occ.insert(Some(msg_num), sender.clone());
                                        Some(sender)
                                    } else if occ
                                        .get(&Some(msg_num))
                                        .map(|a| a.is_closed())
                                        .unwrap_or(false)
                                    {
                                        // Connection is closed and there is no None to replace it
                                        // Remove it for cleanup and report no sender
                                        occ.remove(&Some(msg_num));
                                        None
                                    } else {
                                        None
                                    };
                                    if let Some(sender) = sender {
                                        if sender.capacity() == 0 {
                                            warn!("Reaching limit of channel");
                                            warn!(
                                                "Remaining: {} of {} message space for {} (ID: {})",
                                                sender.capacity(),
                                                sender.max_capacity(),
                                                &msg_num,
                                                &msg_id
                                            );
                                        } else {
                                            trace!(
                                                "Remaining: {} of {} message space for {} (ID: {})",
                                                sender.capacity(),
                                                sender.max_capacity(),
                                                &msg_num,
                                                &msg_id
                                            );
                                        }
                                        if sender.send(Ok(response)).await.is_err() {
                                            occ.remove(&Some(msg_num));
                                        }
                                    } else {
                                        debug!(
                                            "Ignoring uninteresting message id {} (number: {})",
                                            msg_id, msg_num
                                        );
                                        trace!("Contents: {:?}", response);
                                    }
                                }
                                (None, None) => {
                                    debug!(
                                        "Ignoring uninteresting message id {} (number: {})",
                                        msg_id, msg_num
                                    );
                                    trace!("Contents: {:?}", response);
                                }
                            }
                        }
                        Err(e) => {
                            for sub in self.subscribers.num.values() {
                                for sender in sub.values() {
                                    let _ = sender.send(Err(e.clone())).await;
                                }
                            }
                            self.subscribers.num.clear();
                            self.subscribers.id.clear();
                            return Err(e);
                        }
                    }
                }
                PollCommand::AddHandler(msg_id, handler) => {
                    match self.subscribers.id.entry(msg_id) {
                        Entry::Vacant(vac_entry) => {
                            vac_entry.insert(handler);
                        }
                        Entry::Occupied(_) => {
                            return Err(Error::SimultaneousSubscriptionId { msg_id });
                        }
                    };
                }
                PollCommand::RemoveHandler(msg_id) => {
                    self.subscribers.id.remove(&msg_id);
                }
                PollCommand::AddSubscriber(msg_id, msg_num, tx) => {
                    match self
                        .subscribers
                        .num
                        .entry(msg_id)
                        .or_default()
                        .entry(msg_num)
                    {
                        Entry::Vacant(vac_entry) => {
                            vac_entry.insert(tx);
                        }
                        Entry::Occupied(mut occ_entry) => {
                            if occ_entry.get().is_closed() {
                                occ_entry.insert(tx);
                            } else {
                                let _ = tx
                                    .send(Err(Error::SimultaneousSubscription { msg_num }))
                                    .await;
                            }
                        }
                    };
                }
            }
        }
        Ok(())
    }
}

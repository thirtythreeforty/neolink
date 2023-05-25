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
    /// Subscribers based on their Num
    num: BTreeMap<u16, Sender<Result<Bc>>>,
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

    pub async fn subscribe(&self, msg_num: u16) -> Result<BcSubscription> {
        let (tx, rx) = channel(100);
        self.poll_commander
            .send(PollCommand::AddSubscriber(msg_num, tx))
            .await?;
        Ok(BcSubscription::new(rx, msg_num as u32, self))
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
    AddSubscriber(u16, Sender<Result<Bc>>),
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
                PollCommand::Bc(boxed_response) => match *boxed_response {
                    Ok(response) => {
                        let msg_num = response.meta.msg_num;
                        let msg_id = response.meta.msg_id;

                        let mut remove_it_num = false;
                        match (
                            self.subscribers.id.get(&msg_id),
                            self.subscribers.num.get(&msg_num),
                        ) {
                            (Some(occ), _) => {
                                if let Some(reply) = occ(&response).await {
                                    assert!(reply.meta.msg_num == response.meta.msg_num);
                                    self.sink.send(Ok(reply)).await?;
                                }
                            }
                            (None, Some(occ)) => {
                                if occ.capacity() == 0 {
                                    warn!("Reaching limit of channel");
                                    warn!(
                                        "Remaining: {} of {} message space for {} (ID: {})",
                                        occ.capacity(),
                                        occ.max_capacity(),
                                        &msg_num,
                                        &msg_id
                                    );
                                } else {
                                    trace!(
                                        "Remaining: {} of {} message space for {} (ID: {})",
                                        occ.capacity(),
                                        occ.max_capacity(),
                                        &msg_num,
                                        &msg_id
                                    );
                                }
                                if occ.send(Ok(response)).await.is_err() {
                                    remove_it_num = true;
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
                        if remove_it_num {
                            self.subscribers.num.remove(&msg_num);
                        }
                    }
                    Err(e) => {
                        for sub in self.subscribers.num.values() {
                            let _ = sub.send(Err(e.clone())).await;
                        }
                        self.subscribers.num.clear();
                        self.subscribers.id.clear();
                        return Err(e);
                    }
                },
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
                PollCommand::AddSubscriber(msg_num, tx) => {
                    match self.subscribers.num.entry(msg_num) {
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

use super::BcSubscription;
use crate::{bc::model::*, Error, Result};
use futures::sink::{Sink, SinkExt};
use futures::stream::{Stream, StreamExt};
use log::*;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::Mutex;

use tokio::{sync::RwLock, task::JoinSet};

type MsgHandler = Box<dyn Fn(&Bc) -> Option<Bc> + Send + Sync>;
#[derive(Default)]
struct Subscriber {
    /// Subscribers based on their Num
    num: RwLock<BTreeMap<u16, Sender<Bc>>>,
    /// Subscribers based on their ID
    id: RwLock<BTreeMap<u32, MsgHandler>>,
}

pub(crate) type BcConnSink = Box<dyn Sink<Bc, Error = Error> + Send + Sync + Unpin>;
pub(crate) type BcConnSource = Box<dyn Stream<Item = Result<Bc>> + Send + Sync + Unpin>;

/// A shareable connection to a camera.  Handles serialization of messages.  To send/receive, call
/// .[subscribe()] with a message number.  You can use the BcSubscription to send or receive only
/// messages with that number; each incoming message is routed to its appropriate subscriber.
///
/// There can be only one subscriber per kind of message at a time.
pub struct BcConnection {
    sink: Arc<Mutex<BcConnSink>>,
    subscribers: Arc<Subscriber>,
    #[allow(dead_code)] // Not dead we just need to hold a reference to keep it alive
    rx_thread: JoinSet<()>,
}

impl BcConnection {
    pub async fn new(sink: BcConnSink, mut source: BcConnSource) -> Result<BcConnection> {
        let subscribers: Arc<Subscriber> = Default::default();
        let sink = Arc::new(Mutex::new(sink));

        let subs = subscribers.clone();

        let mut rx_thread = JoinSet::new();
        let sink_thread = sink.clone();
        rx_thread.spawn(async move {
            loop {
                let packet = source.next().await;
                let bc = match packet {
                    Some(Ok(bc)) => bc,
                    Some(Err(e)) => {
                        error!("Deserialization error: {:?}", e);
                        break;
                    }
                    None => continue,
                };

                if let Err(e) = Self::poll(bc, &subs, &sink_thread).await {
                    error!("Subscription error: {:?}", e);
                    break;
                }
            }
        });

        Ok(BcConnection {
            sink,
            subscribers,
            rx_thread,
        })
    }

    pub(super) async fn send(&self, bc: Bc) -> crate::Result<()> {
        trace!("send Wait: {:?}", bc);
        self.sink.lock().await.send(bc).await?;
        trace!("send Complete");
        Ok(())
    }

    pub async fn subscribe(&self, msg_num: u16) -> Result<BcSubscription> {
        let (tx, rx) = channel(100);
        match self.subscribers.num.write().await.entry(msg_num) {
            Entry::Vacant(vac_entry) => {
                vac_entry.insert(tx);
            }
            Entry::Occupied(mut occ_entry) => {
                if occ_entry.get().is_closed() {
                    occ_entry.insert(tx);
                } else {
                    return Err(Error::SimultaneousSubscription { msg_num });
                }
            }
        };
        Ok(BcSubscription::new(rx, msg_num as u32, self))
    }

    /// Some messages are initiated by the camera. This creates a handler for them
    /// It requires a closure that will be used to handle the message
    /// and return either None or Some(Bc) reply
    pub async fn handle_msg<T>(&self, msg_id: u32, handler: T) -> Result<()>
    where
        T: Fn(&Bc) -> Option<Bc> + Send + Sync + 'static,
    {
        match self.subscribers.id.write().await.entry(msg_id) {
            Entry::Vacant(vac_entry) => {
                vac_entry.insert(Box::new(handler));
            }
            Entry::Occupied(_) => {
                return Err(Error::SimultaneousSubscriptionId { msg_id });
            }
        };
        Ok(())
    }

    async fn poll(response: Bc, subscribers: &Subscriber, sink: &Mutex<BcConnSink>) -> Result<()> {
        // Don't hold the lock during deserialization so we don't poison the subscribers mutex if
        // something goes wrong
        let msg_num = response.meta.msg_num;
        let msg_id = response.meta.msg_id;

        let mut remove_it_num = false;
        match (
            subscribers.id.read().await.get(&msg_id),
            subscribers.num.read().await.get(&msg_num),
        ) {
            (Some(occ), _) => {
                if let Some(reply) = occ(&response) {
                    assert!(reply.meta.msg_num == response.meta.msg_num);
                    sink.lock().await.send(reply).await?;
                }
            }
            (None, Some(occ)) => {
                trace!("occ.full: {}/{}", occ.capacity(), occ.max_capacity());
                if occ.send(response).await.is_err() {
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
            subscribers.num.write().await.remove(&msg_num);
        }

        Ok(())
    }
}

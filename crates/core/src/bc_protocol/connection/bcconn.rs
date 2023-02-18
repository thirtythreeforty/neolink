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

use tokio::{
    sync::RwLock,
    task::{self, JoinHandle},
};

type Subscriber = Arc<RwLock<BTreeMap<u16, Sender<Bc>>>>;
pub(crate) type BcConnSink = Box<dyn Sink<Bc, Error = Error> + Send + Sync + Unpin>;
pub(crate) type BcConnSource = Box<dyn Stream<Item = Result<Bc>> + Send + Sync + Unpin>;

/// A shareable connection to a camera.  Handles serialization of messages.  To send/receive, call
/// .[subscribe()] with a message number.  You can use the BcSubscription to send or receive only
/// messages with that number; each incoming message is routed to its appropriate subscriber.
///
/// There can be only one subscriber per kind of message at a time.
pub struct BcConnection {
    sink: Arc<Mutex<BcConnSink>>,
    subscribers: Subscriber,
    rx_thread: Mutex<Option<JoinHandle<()>>>,
}

impl BcConnection {
    pub async fn new(sink: BcConnSink, mut source: BcConnSource) -> Result<BcConnection> {
        let subscribers: Subscriber = Default::default();

        let subs = subscribers.clone();

        let rx_thread = task::spawn(async move {
            loop {
                trace!("packet Wait");
                let packet = source.next().await;
                trace!("packet: {:?}", packet);
                let bc = match packet {
                    Some(Ok(bc)) => bc,
                    Some(Err(e)) => {
                        error!("Deserialization error: {:?}", e);
                        break;
                    }
                    None => continue,
                };

                if let Err(e) = Self::poll(bc, &subs).await {
                    error!("Subscription error: {:?}", e);
                    break;
                }
            }
        });

        Ok(BcConnection {
            sink: Arc::new(Mutex::new(sink)),
            subscribers,
            rx_thread: Mutex::new(Some(rx_thread)),
        })
    }

    pub fn stop_polling(&self) {
        if let Some(t) = self.rx_thread.blocking_lock().take() {
            t.abort()
        };
    }

    pub(super) async fn send(&self, bc: Bc) -> crate::Result<()> {
        trace!("send Wait: {:?}", bc);
        self.sink.lock().await.send(bc).await?;
        trace!("send Complete");
        Ok(())
    }

    pub async fn subscribe(&self, msg_num: u16) -> Result<BcSubscription> {
        let (tx, rx) = channel(100);
        match self.subscribers.write().await.entry(msg_num) {
            Entry::Vacant(vac_entry) => vac_entry.insert(tx),
            Entry::Occupied(_) => return Err(Error::SimultaneousSubscription { msg_num }),
        };
        Ok(BcSubscription::new(rx, msg_num, self))
    }

    pub fn unsubscribe(&self, msg_num: u16) -> Result<()> {
        let subs = self.subscribers.clone();
        tokio::task::spawn(async move {
            subs.write().await.remove(&msg_num);
        });

        Ok(())
    }

    async fn poll(response: Bc, subscribers: &Subscriber) -> Result<()> {
        // Don't hold the lock during deserialization so we don't poison the subscribers mutex if
        // something goes wrong
        let msg_num = response.meta.msg_num;
        let msg_id = response.meta.msg_id;

        let mut remove_it = false;
        match subscribers.read().await.get(&msg_num) {
            Some(occ) => {
                if occ.send(response).await.is_err() {
                    // Exceedingly unlikely, unless you mishandle the subscription object
                    warn!("Subscriber to ID {} dropped their channel", msg_id);
                    remove_it = true;
                }
            }
            None => {
                debug!("Ignoring uninteresting message num {}", msg_id);
                trace!("Contents: {:?}", response);
            }
        }
        if remove_it {
            subscribers.write().await.remove(&msg_num);
        }

        Ok(())
    }
}

impl Drop for BcConnection {
    fn drop(&mut self) {
        debug!("Shutting down BcConnection...");
        if let Some(t) = self.rx_thread.get_mut().take() {
            t.abort()
        };
    }
}

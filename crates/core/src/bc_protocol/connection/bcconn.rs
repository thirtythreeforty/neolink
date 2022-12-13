use super::{BcSource, BcSubscription, Error, Result, TcpSource};
use crate::bc;
use crate::bc::model::*;
use crossbeam_channel::{unbounded, Receiver, Sender};
use log::*;
use socket2::{Domain, Socket, Type};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::error::Error as StdErr; // Just need the traits
use std::io::{Error as IoError, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A shareable connection to a camera.  Handles serialization of messages.  To send/receive, call
/// .[subscribe()] with a message number.  You can use the BcSubscription to send or receive only
/// messages with that number; each incoming message is routed to its appropriate subscriber.
///
/// There can be only one subscriber per kind of message at a time.
pub struct BcConnection {
    sink: Arc<Mutex<BcSource>>,
    subscribers: Arc<Mutex<BTreeMap<u16, Sender<Bc>>>>,
    rx_thread: Option<JoinHandle<()>>,
    // Arc<Mutex<EncryptionProtocol>> because it is shared between context
    // and connection for deserialisation and serialistion respectivly
    encryption_protocol: Arc<Mutex<EncryptionProtocol>>,
    poll_abort: Arc<AtomicBool>,
    keep_alive_msg: Arc<Mutex<Option<Bc>>>,
}

impl BcConnection {
    pub fn new(source: BcSource) -> Result<Self> {
        let subscribers: Arc<Mutex<BTreeMap<u16, Sender<Bc>>>> = Default::default();

        let mut subs = subscribers.clone();

        let encryption_protocol = Arc::new(Mutex::new(EncryptionProtocol::Unencrypted));
        let connections_encryption_protocol = encryption_protocol.clone();
        let poll_abort = Arc::new(AtomicBool::new(false));
        let poll_abort_rx = poll_abort.clone();
        let mut conn = source.try_clone()?;
        let keep_alive_msg: Arc<Mutex<Option<Bc>>> = Arc::new(Mutex::new(None));
        let connections_keep_alive_msg = keep_alive_msg.clone();
        let rx_thread = std::thread::spawn(move || {
            let keep_alive_encryption_protocol = connections_encryption_protocol.clone();
            let mut context = BcContext::new(connections_encryption_protocol);
            let mut result;
            let mut last_keep_alive = Instant::now();
            let keep_alive_time = Duration::from_millis(500);
            loop {
                result = Self::poll(&mut context, &conn, &mut subs, &connections_keep_alive_msg);
                if poll_abort_rx.load(Ordering::Relaxed) {
                    break; // Poll has been aborted by request usally during disconnect
                }
                if let Err(e) = result {
                    error!("Deserialization error: {}", e);
                    let mut cause = e.source();
                    while let Some(e) = cause {
                        error!("caused by: {}", e);
                        cause = e.source();
                    }
                    break;
                }
                // Send a udp keep alive if set
                if last_keep_alive.elapsed() > keep_alive_time {
                    last_keep_alive = Instant::now();
                    if let Ok(lock) = connections_keep_alive_msg.try_lock() {
                        if let Some(keep_alive_msg) = lock.as_ref() {
                            let _ = keep_alive_msg
                                .serialize(&conn, &keep_alive_encryption_protocol.lock().unwrap());
                            let _ = conn.flush();
                        }
                    }
                }
            }
        });

        Ok(BcConnection {
            sink: Arc::new(Mutex::new(source)),
            subscribers,
            rx_thread: Some(rx_thread),
            encryption_protocol,
            poll_abort,
            keep_alive_msg,
        })
    }

    pub fn stop_polling(&self) {
        self.poll_abort.store(true, Ordering::Relaxed);
    }

    pub(super) fn send(&self, bc: Bc) -> Result<()> {
        bc.serialize(&*self.sink.lock().unwrap(), &self.get_encrypted())?;
        let _ = self.sink.lock().unwrap().flush();
        Ok(())
    }

    #[allow(clippy::significant_drop_in_scrutinee)]
    pub fn subscribe(&self, msg_num: u16) -> Result<BcSubscription> {
        let (tx, rx) = unbounded();
        match self.subscribers.lock().unwrap().entry(msg_num) {
            Entry::Vacant(vac_entry) => vac_entry.insert(tx),
            Entry::Occupied(_) => return Err(Error::SimultaneousSubscription { msg_num }),
        };
        Ok(BcSubscription::new(rx, msg_num, self))
    }

    pub fn unsubscribe(&self, msg_num: u16) -> Result<()> {
        self.subscribers.lock().unwrap().remove(&msg_num);
        Ok(())
    }

    pub fn set_keep_alive_msg(&self, msg: Bc) {
        *self.keep_alive_msg.lock().unwrap() = Some(msg);
    }

    pub fn set_encrypted(&self, value: EncryptionProtocol) {
        *(self.encryption_protocol.lock().unwrap()) = value;
    }

    pub fn get_encrypted(&self) -> EncryptionProtocol {
        (*self.encryption_protocol.lock().unwrap()).clone()
    }

    pub fn is_udp(&self) -> bool {
        self.sink.lock().unwrap().is_udp()
    }

    #[allow(clippy::significant_drop_in_scrutinee)]
    fn poll(
        context: &mut BcContext,
        connection: &BcSource,
        subscribers: &mut Arc<Mutex<BTreeMap<u16, Sender<Bc>>>>,
        connections_keep_alive_msg: &Arc<Mutex<Option<Bc>>>,
    ) -> Result<()> {
        // Don't hold the lock during deserialization so we don't poison the subscribers mutex if
        // something goes wrong
        let response = Bc::deserialize(context, connection).map_err(|err| {
            // If the connection hangs up, hang up on all subscribers
            subscribers.lock().unwrap().clear();
            err
        })?;
        let msg_num = response.meta.msg_num;
        let msg_id = response.meta.msg_id;

        let mut locked_subs = subscribers.lock().unwrap();
        match locked_subs.entry(msg_num) {
            Entry::Occupied(mut occ) => {
                if occ.get_mut().send(response).is_err() {
                    // Exceedingly unlikely, unless you mishandle the subscription object
                    warn!("Subscriber to ID {} dropped their channel", msg_id);
                    occ.remove();
                }
            }
            Entry::Vacant(_) => {
                if msg_id != MSG_ID_UDP_KEEP_ALIVE {
                    debug!("Ignoring uninteresting message num {}", msg_id);
                    trace!("Contents: {:?}", response);
                } else {
                    // This is a keep alive message let see what the camera says about it
                    if response.meta.response_code != 200 {
                        // Camera dosen't support the current keep alive message stop sending them
                        if let Ok(mut lock) = connections_keep_alive_msg.try_lock() {
                            *lock = None;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl Drop for BcConnection {
    fn drop(&mut self) {
        debug!("Shutting down BcConnection...");
        match self
            .rx_thread
            .take()
            .expect("rx_thread join handle should always exist")
            .join()
        {
            Ok(_) => {
                debug!("Shutdown finished OK");
            }
            Err(e) => {
                error!("Receiving thread panicked: {:?}", e);
            }
        }
    }
}

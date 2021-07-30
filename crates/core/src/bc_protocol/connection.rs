use crate::bc;
use crate::bc::model::*;
use err_derive::Error;
use log::*;
use socket2::{Domain, Socket, Type};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::error::Error as StdErr; // Just need the traits
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// A shareable connection to a camera.  Handles serialization of messages.  To send/receive, call
/// .subscribe() with a message ID.  You can use the BcSubscription to send or receive only
/// messages with that ID; each incoming message is routed to its appropriate subscriber.
///
/// There can be only one subscriber per kind of message at a time.
pub struct BcConnection {
    connection: Arc<Mutex<TcpStream>>,
    subscribers: Arc<Mutex<BTreeMap<u32, Sender<Bc>>>>,
    rx_thread: Option<JoinHandle<()>>,
    // Arc<Mutex<EncryptionProtocol>> because it is shared between context
    // and connection for deserialisation and serialistion respectivly
    encryption_protocol: Arc<Mutex<EncryptionProtocol>>,
}

pub struct BcSubscription<'a> {
    pub rx: Receiver<Bc>,
    msg_id: u32,
    conn: &'a BcConnection,
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display = "Communication error")]
    CommunicationError(#[error(source)] std::io::Error),

    #[error(display = "Deserialization error")]
    DeserializationError(#[error(source)] bc::de::Error),

    #[error(display = "Serialization error")]
    SerializationError(#[error(source)] bc::ser::Error),

    #[error(display = "Simultaneous subscription")]
    SimultaneousSubscription { msg_id: u32 },
}

impl BcConnection {
    pub fn new(addr: SocketAddr, timeout: Duration) -> Result<BcConnection> {
        let tcp_conn = connect_to(addr, timeout)?;
        let subscribers: Arc<Mutex<BTreeMap<u32, Sender<Bc>>>> = Default::default();

        let mut subs = subscribers.clone();
        let conn = tcp_conn.try_clone()?;

        let encryption_protocol = Arc::new(Mutex::new(EncryptionProtocol::Unencrypted));
        let connections_encryption_protocol = encryption_protocol.clone();
        let rx_thread = std::thread::spawn(move || {
            let mut context = BcContext::new(connections_encryption_protocol);
            let mut result;
            while {
                result = BcConnection::poll(&mut context, &conn, &mut subs);
                result.is_ok()
            } {}
            let e = result.unwrap_err();
            error!("Deserialization error: {}", e);
            let mut cause = e.source();
            while let Some(e) = cause {
                error!("caused by: {}", e);
                cause = e.source();
            }
        });

        Ok(BcConnection {
            connection: Arc::new(Mutex::new(tcp_conn)),
            subscribers,
            rx_thread: Some(rx_thread),
            encryption_protocol,
        })
    }

    pub fn subscribe(&self, msg_id: u32) -> Result<BcSubscription> {
        let (tx, rx) = channel();
        match self.subscribers.lock().unwrap().entry(msg_id) {
            Entry::Vacant(vac_entry) => vac_entry.insert(tx),
            Entry::Occupied(_) => return Err(Error::SimultaneousSubscription { msg_id }),
        };
        Ok(BcSubscription {
            rx,
            conn: self,
            msg_id,
        })
    }

    fn poll(
        context: &mut BcContext,
        connection: &TcpStream,
        subscribers: &mut Arc<Mutex<BTreeMap<u32, Sender<Bc>>>>,
    ) -> Result<()> {
        // Don't hold the lock during deserialization so we don't poison the subscribers mutex if
        // something goes wrong

        let response = Bc::deserialize(context, connection).map_err(|err| {
            // If the connection hangs up, hang up on all subscribers
            subscribers.lock().unwrap().clear();
            err
        })?;
        let msg_id = response.meta.msg_id;

        let mut locked_subs = subscribers.lock().unwrap();
        match locked_subs.entry(msg_id) {
            Entry::Occupied(mut occ) => {
                if occ.get_mut().send(response).is_err() {
                    // Exceedingly unlikely, unless you mishandle the subscription object
                    warn!("Subscriber to ID {} dropped their channel", msg_id);
                    occ.remove();
                }
            }
            Entry::Vacant(_) => {
                debug!("Ignoring uninteresting message ID {}", msg_id);
                trace!("Contents: {:?}", response);
            }
        }

        Ok(())
    }

    pub fn set_encrypted(&self, value: EncryptionProtocol) {
        *(self.encryption_protocol.lock().unwrap()) = value;
    }

    pub fn get_encrypted(&self) -> EncryptionProtocol {
        (*self.encryption_protocol.lock().unwrap()).clone()
    }
}

impl Drop for BcConnection {
    fn drop(&mut self) {
        debug!("Shutting down BcConnection...");
        let _ = self.connection.lock().unwrap().shutdown(Shutdown::Both);
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

impl<'a> BcSubscription<'a> {
    pub fn send(&self, bc: Bc) -> Result<()> {
        assert!(bc.meta.msg_id == self.msg_id);

        bc.serialize(
            &*self.conn.connection.lock().unwrap(),
            &self.conn.get_encrypted(),
        )?;
        Ok(())
    }
}

/// Makes it difficult to avoid unsubscribing when you're finished
impl<'a> Drop for BcSubscription<'a> {
    fn drop(&mut self) {
        self.conn.subscribers.lock().unwrap().remove(&self.msg_id);
    }
}

/// Helper to create a TcpStream with a connect timeout
fn connect_to(addr: SocketAddr, timeout: Duration) -> Result<TcpStream> {
    let socket = match addr {
        SocketAddr::V4(_) => Socket::new(Domain::ipv4(), Type::stream(), None)?,
        SocketAddr::V6(_) => {
            let s = Socket::new(Domain::ipv6(), Type::stream(), None)?;
            s.set_only_v6(false)?;
            s
        }
    };

    socket.set_keepalive(Some(timeout))?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;
    socket.connect_timeout(&addr.into(), timeout)?;

    Ok(socket.into_tcp_stream())
}

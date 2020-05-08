use crate::bc;
use crate::bc::model::*;
use err_derive::Error;
use log::*;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::net::{TcpStream, SocketAddr};

/// A shareable connection to a camera.  Handles serialization of messages.
pub struct BcConnection {
    connection: Arc<TcpStream>,
    subscribers: Arc<Mutex<BTreeMap<u32, Sender<Bc>>>>,
}

pub struct BcSubscription<'a> {
    pub rx: Receiver<Bc>,
    pub msg_id: u32,
    conn: &'a BcConnection,
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display="Communication error")]
    CommunicationError(#[error(source)] std::io::Error),

    #[error(display="Deserialization error")]
    DeserializationError(#[error(source)] bc::de::Error),

    #[error(display="Serialization error")]
    SerializationError(#[error(source)] bc::ser::Error),
}

impl BcConnection {
    pub fn new(addr: SocketAddr) -> Result<BcConnection> {
        let bc_conn = BcConnection {
            connection: Arc::new(TcpStream::connect(addr)?),
            subscribers: Default::default(),
        };

        let mut conn = bc_conn.connection.clone();
        let mut subs = bc_conn.subscribers.clone();

        let rx_poller = std::thread::spawn(move || {
            let mut context = BcContext::new();
            while let Ok(_) = BcConnection::poll(&mut context, &mut conn, &mut subs) {}
        });

        Ok(bc_conn)
    }

    pub fn subscribe(&self, msg_id: u32) -> Result<BcSubscription> {
        let (tx, rx) = channel();
        {
            let mut locked_subs = self.subscribers.lock().unwrap();
            locked_subs.insert(msg_id, tx);
        }
        Ok(BcSubscription { rx, conn: self, msg_id })
    }

    fn poll(
        context: &mut BcContext,
        connection: &mut Arc<TcpStream>,
        subscribers: &mut Arc<Mutex<BTreeMap<u32, Sender<Bc>>>>
    ) -> Result<()> {
        // TODO if the connection hangs up, hang up on all subscribers and delete our TcpStream
        let response = Bc::deserialize(context, &**connection)?;
        let msg_id = response.meta.msg_id;

        let mut locked_subs = subscribers.lock().unwrap();
        match locked_subs.entry(msg_id) {
            Entry::Occupied(mut occ) => {
                if let Err(_) = occ.get_mut().send(response) {
                    occ.remove();
                    warn!("Subscriber to ID {} dropped their channel", msg_id);
                }
            }
            Entry::Vacant(_) => {
                info!("Ignoring uninteresting message ID {}", msg_id);
                debug!("Contents: {:?}", response);
            }
        }

        Ok(())
    }
}

impl<'a> BcSubscription<'a> {
    pub fn send(&self, bc: Bc) -> Result<()> {
        // TODO Use channel for writing, too
        bc.serialize(&*self.conn.connection)?;
        Ok(())
    }
}

/// Makes it difficult to avoid unsubscribing when you're finished
impl<'a> Drop for BcSubscription<'a> {
    fn drop(&mut self) {
        self.conn.subscribers.lock().unwrap().remove(&self.msg_id);
    }
}

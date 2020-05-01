use crate::bc;
use crate::bc::{model::*};
use err_derive::Error;
use log::*;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Mutex;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::net::{TcpStream, SocketAddr};

pub struct BcConnection {
    connection: TcpStream,
    context: BcContext,
    subscribers: Mutex<BTreeMap<u32, Sender<Bc>>>,
}

pub struct BcSubscription<'a> {
    pub rx: Receiver<Bc>,
    conn: &'a BcConnection,
    msg_id: u32,
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
        let connection = TcpStream::connect(addr)?;

        Ok(BcConnection {
            connection,
            context: BcContext::new(),
            subscribers: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn subscribe(&self, msg_id: u32) -> Result<BcSubscription> {
        let (tx, rx) = channel();
        {
            let mut locked_subs = self.subscribers.lock().unwrap();
            locked_subs.insert(msg_id, tx);
        }
        Ok(BcSubscription { rx, conn: self, msg_id })
    }

    pub fn poll(&mut self) -> Result<()> {
        // TODO if the connection hangs up, hang up on all subscribers and delete our TcpStream
        let response = Bc::deserialize(&mut self.context, &self.connection)?;
        let msg_id = response.meta.msg_id;

        let mut locked_subs = self.subscribers.lock().unwrap();
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
        bc.serialize(&self.conn.connection)?;
        Ok(())
    }
}

/// Makes it difficult to avoid unsubscribing when you're finished
impl<'a> Drop for BcSubscription<'a> {
    fn drop(&mut self) {
        self.conn.subscribers.lock().unwrap().remove(&self.msg_id);
    }
}

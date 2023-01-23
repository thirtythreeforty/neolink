//! This module handles connections and subscribers
//!
//! This includes a tcp and udp connections. As well
//! as subscribers to binary streams that are encoded
//! in the bc packets.
//!
use crate::bc;
use crate::bc::model::*;
use crate::bcmedia;
use crate::bcudp;
use crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use err_derive::Error;
use log::*;
use socket2::{Domain, Socket, Type};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::error::Error as StdErr; // Just need the traits
use std::net::{SocketAddr, TcpStream};
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};

use std::thread::JoinHandle;
use std::time::Duration;

mod bcconn;
mod bcsource;
mod bcsub;
mod binarysub;
mod filesub;
mod tcpconn;
mod udpconn;

pub(crate) use self::{
    bcconn::BcConnection, bcsource::BcSource, bcsub::BcSubscription, binarysub::BinarySubscriber,
    filesub::FileSubscriber, tcpconn::TcpSource, udpconn::UdpSource,
};

#[derive(Debug, Error, Clone)]
pub enum Error {
    #[error(display = "Communication error")]
    Communication(#[error(source)] Arc<std::io::Error>),

    #[error(display = "Communication Reciever error")]
    RecvCommunication(#[error(source)] RecvError),

    #[error(display = "Deserialization error")]
    Deserialization(#[error(source)] bc::de::Error),

    #[error(display = "BcMedia Deserialization error (conn)")]
    BcMediaDeserialization(#[error(source)] bcmedia::de::Error),

    #[error(display = "Serialization error")]
    Serialization(#[error(source)] bc::ser::Error),

    #[error(display = "UDP Deserialization error")]
    UdpDeserialization(#[error(source)] bcudp::de::Error),

    #[error(display = "UDP Serialization error")]
    UdpSerialization(#[error(source)] bcudp::ser::Error),

    #[error(display = "Simultaneous subscription")]
    SimultaneousSubscription { msg_num: u16 },

    #[error(display = "Timeout")]
    Timeout,

    #[error(display = "This connection type in unsupported")]
    UnsupportedConnection,

    #[error(display = "Camera Not Findable")]
    ConnectionUnavaliable,
}

impl From<std::io::Error> for Error {
    fn from(k: std::io::Error) -> Self {
        Error::Communication(std::sync::Arc::new(k))
    }
}

type Result<T> = std::result::Result<T, Error>;

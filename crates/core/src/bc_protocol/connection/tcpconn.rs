use super::{BcSubscription, Error, Result};
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
use std::time::Duration;

type IoResult<T> = std::result::Result<T, IoError>;

pub struct TcpSource {
    stream: TcpStream,
}

impl TcpSource {
    pub fn new(addr: SocketAddr, timeout: Duration) -> Result<TcpSource> {
        let tcp_conn = connect_to(addr, timeout)?;

        Ok(Self { stream: tcp_conn })
    }

    pub fn try_clone(&self) -> IoResult<Self> {
        Ok(Self {
            stream: self.stream.try_clone()?,
        })
    }
}

impl Read for TcpSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        self.stream.read(buf)
    }
}

impl Write for TcpSource {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> IoResult<()> {
        self.stream.flush()
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

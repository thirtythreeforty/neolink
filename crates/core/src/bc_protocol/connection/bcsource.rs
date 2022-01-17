use super::{Result, TcpSource, UdpSource};
use std::io::{Error as IoError, ErrorKind, Read, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type IoResult<T> = std::result::Result<T, IoError>;

pub enum BcSource {
    Tcp(Mutex<TcpSource>),
    Udp(Mutex<UdpSource>),
}

impl BcSource {
    pub fn is_udp(&self) -> bool {
        matches!(self, BcSource::Udp(_))
    }

    pub fn new_tcp(addr: SocketAddr, timeout: Duration) -> Result<Self> {
        let source = TcpSource::new(addr, timeout)?;
        Ok(BcSource::Tcp(Mutex::new(source)))
    }

    pub fn new_udp(uid: &str, timeout: Duration) -> Result<Self> {
        let source = UdpSource::new(uid, timeout)?;
        Ok(BcSource::Udp(Mutex::new(source)))
    }

    pub fn try_clone(&self) -> IoResult<Self> {
        match self {
            BcSource::Tcp(source) => match &mut source.try_lock() {
                Ok(locked) => Ok(BcSource::Tcp(Mutex::new(locked.try_clone()?))),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock tcp for clone",
                )),
            },

            BcSource::Udp(source) => match &mut source.try_lock() {
                Ok(locked) => Ok(BcSource::Udp(Mutex::new(locked.try_clone()?))),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock udp for clone",
                )),
            },
        }
    }
}

impl Read for BcSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        match self {
            BcSource::Tcp(source) => match &mut source.get_mut() {
                Ok(locked) => Ok(locked.read(buf)?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock tcp for read",
                )),
            },

            BcSource::Udp(source) => match &mut source.get_mut() {
                Ok(locked) => Ok(locked.read(buf)?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock udp for read",
                )),
            },
        }
    }
}

impl Read for &BcSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        match self {
            BcSource::Tcp(source) => match &mut source.try_lock() {
                Ok(locked) => Ok(locked.read(buf)?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock tcp for &read",
                )),
            },

            BcSource::Udp(source) => match &mut source.try_lock() {
                Ok(locked) => Ok(locked.read(buf)?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock udp for &read",
                )),
            },
        }
    }
}

impl Write for BcSource {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        match self {
            BcSource::Tcp(source) => match &mut source.get_mut() {
                Ok(locked) => Ok(locked.write(buf)?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock tcp for write",
                )),
            },

            BcSource::Udp(source) => match &mut source.get_mut() {
                Ok(locked) => Ok(locked.write(buf)?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock udp for write",
                )),
            },
        }
    }

    fn flush(&mut self) -> IoResult<()> {
        match self {
            BcSource::Tcp(source) => match &mut source.get_mut() {
                Ok(locked) => Ok(locked.flush()?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock tcp for flush",
                )),
            },

            BcSource::Udp(source) => match &mut source.get_mut() {
                Ok(locked) => Ok(locked.flush()?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock tcp udp for flush",
                )),
            },
        }
    }
}

impl Write for &BcSource {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        match self {
            BcSource::Tcp(source) => match &mut source.try_lock() {
                Ok(locked) => Ok(locked.write(buf)?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock tcp for write",
                )),
            },

            BcSource::Udp(source) => match &mut source.try_lock() {
                Ok(locked) => Ok(locked.write(buf)?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock udp for write",
                )),
            },
        }
    }

    fn flush(&mut self) -> IoResult<()> {
        match self {
            BcSource::Tcp(source) => match &mut source.try_lock() {
                Ok(locked) => Ok(locked.flush()?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock tcp for &flush",
                )),
            },

            BcSource::Udp(source) => match &mut source.try_lock() {
                Ok(locked) => Ok(locked.flush()?),
                Err(_) => Err(IoError::new(
                    ErrorKind::WouldBlock,
                    "Unable to lock udp for &flush",
                )),
            },
        }
    }
}

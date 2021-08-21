/// The module will handle a udp connection
///
/// Because of the complexity it is seperated into modules
///
/// discover module handles the initial handshake which discovers
/// the ip from the uid and reconnects
///
/// transmit handles the sending and recieving of data through the socket
/// this includes the BcUdp wrapping and the acknoledgements
///
use super::{Error, Result};
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use lazy_static::lazy_static;
use log::*;
use rand::{seq::SliceRandom, thread_rng, Rng};
use std::{
    io::{BufRead, Error as IoError, ErrorKind, Read, Result as IoResult, Write},
    net::{SocketAddr, UdpSocket},
    sync::Arc,
    time::Duration,
};
use time::OffsetDateTime;

mod aborthandle;
mod discover;
mod transmit;

use aborthandle::*;
use discover::*;
use transmit::*;

// How long to set the socket wait
// This is kept short so as to not block the read and write parts
const SOCKET_WAIT_TIME: Duration = Duration::from_millis(50);
// How long to wait between retransmits when no reply is recieved
const WAIT_TIME: Duration = Duration::from_millis(500);
// The maximum data size including header
//
// TODO: Maybe use path mtu discovery (although reolinks seems to just use this constant)
const MTU: u32 = 1030;

lazy_static! {
    static ref P2P_RELAY_HOSTNAMES: [&'static str; 10] = [
        "p2p.reolink.com",
        "p2p1.reolink.com",
        "p2p2.reolink.com",
        "p2p3.reolink.com",
        "p2p6.reolink.com",
        "p2p7.reolink.com",
        "p2p8.reolink.com",
        "p2p9.reolink.com",
        "p2p14.reolink.com",
        "p2p15.reolink.com",
    ];
}

pub struct UdpSource {
    outgoing: Sender<Vec<u8>>,
    incoming: Receiver<Vec<u8>>,
    aborter: AbortHandle,
    timeout: Duration,
    mtu: u32,

    read_buffer: Buffered,
    write_buffer: Buffered,
}

impl UdpSource {
    pub fn new(uid: &str, timeout: Duration) -> Result<Self> {
        let (outgoing, from_outgoing) = unbounded();
        let (to_incoming, incoming) = unbounded();
        let aborter = AbortHandle::new();

        Self::start_polling(uid, timeout, &aborter, to_incoming, from_outgoing)?;

        Ok(Self {
            outgoing,
            incoming,
            aborter,
            timeout,
            mtu: MTU,

            read_buffer: Default::default(),
            write_buffer: Default::default(),
        })
    }

    fn start_polling(
        uid: &str,
        timeout: Duration,
        aborter: &AbortHandle,
        to_incoming: Sender<Vec<u8>>,
        from_outgoing: Receiver<Vec<u8>>,
    ) -> Result<()> {
        let socket = Self::get_socket(SOCKET_WAIT_TIME)?;
        let allow_remote = true;
        let discovery_result = Arc::new(UdpDiscover::discover_from_uuid(
            &socket,
            uid,
            timeout,
            allow_remote,
        )?);
        socket.connect(discovery_result.address)?;
        let transmit = Arc::new(UdpTransmit::new());

        let thread_aborter = aborter.clone();
        let thread_transmit = transmit.clone();
        let thread_socket = socket.try_clone()?;
        let thread_discovery_result = discovery_result.clone();

        // Poll Read
        std::thread::spawn(move || {
            while !thread_aborter.is_aborted() {
                if let Err(err) = thread_transmit.poll_read(
                    &thread_socket,
                    &(*thread_discovery_result),
                    &to_incoming,
                ) {
                    if !thread_aborter.is_aborted() {
                        match err {
                            TransmitError::Disc => {
                                error!("Camera requested disconnect");
                                thread_aborter.abort();
                            }
                            e => {
                                error!("Error during Udp Transmit: {:?}", e);
                                thread_aborter.abort();
                            }
                        }
                    }
                }
            }
            error!("Udp read poll aborted");
        });

        let thread_aborter = aborter.clone();
        let thread_socket = socket.try_clone()?;
        let thread_transmit = transmit;
        let thread_discovery_result = discovery_result;

        // Poll Write
        std::thread::spawn(move || {
            let mut outgoing_history = Default::default();
            while !thread_aborter.is_aborted() {
                if let Err(err) = thread_transmit.poll_write(
                    &thread_socket,
                    &(*thread_discovery_result),
                    &from_outgoing,
                    &mut outgoing_history,
                ) {
                    if !thread_aborter.is_aborted() {
                        match err {
                            TransmitError::Disc => {
                                error!("Camera requested disconnect");
                                thread_aborter.abort();
                            }
                            e => {
                                error!("Error during Udp Transmit: {:?}", e);
                                thread_aborter.abort();
                            }
                        }
                    }
                }
            }
            error!("Udp write poll aborted");
        });

        Ok(())
    }

    pub fn try_clone(&self) -> IoResult<Self> {
        Ok(Self {
            outgoing: self.outgoing.clone(),
            incoming: self.incoming.clone(),
            aborter: self.aborter.clone(),
            timeout: self.timeout,
            mtu: self.mtu,

            // New buffer so they don't pollute each other
            read_buffer: Default::default(),
            write_buffer: Default::default(),
        })
    }

    fn stop_polling(&self) {
        self.aborter.abort();
    }

    fn get_socket(timeout: Duration) -> Result<UdpSocket> {
        // Select a random port to bind to
        let mut ports: Vec<u16> = (53500..54000).into_iter().collect();
        let mut rng = thread_rng();
        ports.shuffle(&mut rng);

        let addrs: Vec<_> = ports
            .iter()
            .map(|&port| SocketAddr::from(([0, 0, 0, 0], port)))
            .collect();
        let socket = UdpSocket::bind(&addrs[..])?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        socket.set_nonblocking(false)?;
        socket.set_broadcast(true)?;
        Ok(socket)
    }
}

// Ensuring polling stops
impl Drop for UdpSource {
    fn drop(&mut self) {
        self.stop_polling();
    }
}

#[derive(Default)]
struct Buffered {
    buffer: Vec<u8>,
    consumed: usize,
}

impl Read for UdpSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.aborter.is_aborted() {
            return Err(IoError::new(
                ErrorKind::ConnectionAborted,
                "Connection dropped",
            ));
        }
        let buffer = self.fill_buf()?;
        let amt = std::cmp::min(buf.len(), buffer.len());

        // First check if the amount of bytes we want to read is small:
        // `copy_from_slice` will generally expand to a call to `memcpy`, and
        // for a single byte the overhead is significant.
        if amt == 1 {
            buf[0] = buffer[0];
        } else {
            buf[..amt].copy_from_slice(&buffer[..amt]);
        }

        self.consume(amt);

        Ok(amt)
    }
}

impl BufRead for UdpSource {
    fn fill_buf(&mut self) -> IoResult<&[u8]> {
        const CLEAR_CONSUMED_AT: usize = 1024;
        // This is a trade off between caching too much dead memory
        // and calling the drain method too often
        if self.read_buffer.consumed > CLEAR_CONSUMED_AT {
            let _ = self
                .read_buffer
                .buffer
                .drain(0..self.read_buffer.consumed)
                .collect::<Vec<u8>>();
            self.read_buffer.consumed = 0;
        }
        if self.read_buffer.buffer.len() <= self.read_buffer.consumed {
            // Get next packet of the read queue
            match self.incoming.recv_timeout(self.timeout) {
                Ok(buf) => self.read_buffer.buffer.extend(buf),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(IoError::new(
                        ErrorKind::UnexpectedEof,
                        "Connection timedout when reading from udp channel",
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(IoError::new(
                        ErrorKind::UnexpectedEof,
                        "Connection dropped when reading from udp channel",
                    ));
                }
            }
        }

        Ok(&self.read_buffer.buffer.as_slice()[self.read_buffer.consumed..])
    }

    fn consume(&mut self, amt: usize) {
        assert!(self.read_buffer.consumed + amt <= self.read_buffer.buffer.len());
        self.read_buffer.consumed += amt;
    }
}

const UDPDATA_HEADER_SIZE: usize = 20;
impl Write for UdpSource {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        if self.aborter.is_aborted() {
            return Err(IoError::new(
                std::io::ErrorKind::ConnectionAborted,
                "Connection dropped",
            ));
        }
        self.write_buffer.buffer.extend(buf.to_vec());
        if self.write_buffer.buffer.len() > self.mtu as usize - UDPDATA_HEADER_SIZE {
            let _ = self.flush();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        if self.aborter.is_aborted() {
            return Err(IoError::new(
                std::io::ErrorKind::ConnectionAborted,
                "Connection dropped",
            ));
        }
        for chunk in self
            .write_buffer
            .buffer
            .chunks(self.mtu as usize - UDPDATA_HEADER_SIZE)
        {
            if self.outgoing.send(chunk.to_vec()).is_err() {
                return Err(IoError::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "Connection dropped",
                ));
            }
        }
        self.write_buffer.buffer.clear();
        Ok(())
    }
}

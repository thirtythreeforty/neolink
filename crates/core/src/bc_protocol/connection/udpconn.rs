use super::{BcSubscription, Error, Result};
use crate::bc;
use crate::bc::model::*;
use crate::bcudp;
use crate::bcudp::{model::*, xml::*};
use crate::RX_TIMEOUT;
use log::*;
use rand::{seq::SliceRandom, thread_rng, Rng};
use socket2::{Domain, Socket, Type};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::error::Error as StdErr; // Just need the traits
use std::io::{BufRead, Error as IoError, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{channel, Receiver, Sender},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::Duration;
use time::OffsetDateTime;

type IoResult<T> = std::result::Result<T, IoError>;

const WAIT_TIME: Duration = Duration::from_millis(500);

pub struct UdpSource {
    socket: UdpSocket,
    address: SocketAddr,
    client_id: u32,
    camera_id: u32,
    mtu: u32,
    outgoing: Arc<Mutex<HashMap<u32, QueuedMessage>>>,
    incoming: Arc<Mutex<HashMap<u32, QueuedMessage>>>,
    aborter: AbortHandle,
    next_to_consume: Arc<AtomicU32>,
    next_send_message: Arc<AtomicU32>,
    // Buffered because a single read might not read a whole packet
    read_buffer: Buffered,
    write_buffer: Buffered,
}

#[derive(Default, Clone)]
struct Buffered {
    buffer: Vec<u8>,
    consumed: usize,
}

#[derive(Debug)]
struct QueuedMessage {
    buf: Vec<u8>,
    time_last_tried: Option<OffsetDateTime>,
}

struct DiscoveryResult {
    socket: UdpSocket,
    address: SocketAddr,
    client_id: u32,
    camera_id: u32,
    mtu: u32,
}

#[derive(Clone)]
struct AbortHandle {
    aborted: Arc<AtomicBool>,
}

impl AbortHandle {
    fn new() -> Self {
        Self {
            aborted: Arc::new(AtomicBool::new(false)),
        }
    }

    fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }
}

impl UdpSource {
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
        socket.set_broadcast(true)?;
        Ok(socket)
    }

    fn retrying_send_to<T: ToSocketAddrs>(
        socket: &UdpSocket,
        buf: &[u8],
        addr: T,
    ) -> IoResult<(usize, AbortHandle)> {
        let addr: SocketAddr = addr.to_socket_addrs()?.next().unwrap();
        let handle = AbortHandle::new();
        let bytes_send = socket.send_to(&buf, &addr)?;

        let thread_buffer = buf.to_vec();
        let thread_handle = handle.clone();
        let thread_socket = socket.try_clone()?;
        let thread_addr = addr;

        std::thread::spawn(move || {
            std::thread::sleep(WAIT_TIME);
            while !thread_handle.is_aborted() {
                let _ = thread_socket.send_to(&thread_buffer[..], &thread_addr);
                std::thread::sleep(WAIT_TIME);
            }
        });

        Ok((bytes_send, handle))
    }

    fn discover_from_uuid(uid: &str, timeout: Duration) -> Result<DiscoveryResult> {
        let start_time = OffsetDateTime::now_utc();

        let socket = Self::get_socket(timeout)?;

        let mut rng = thread_rng();
        // If tid is too large it will overflow during encrypt so we just use a random u8
        let tid: u32 = (rng.gen::<u8>()) as u32;
        let client_id: u32 = rng.gen();
        let local_addr = socket.local_addr()?;
        let port = local_addr.port();

        // TODO: Path MTU discovery
        let mtu = 1350;

        let msg = BcUdp::Discovery(UdpDiscovery {
            tid,
            payload: UdpXml {
                c2d_c: Some(C2dC {
                    uid: uid.to_string(),
                    cli: ClientList { port: port as u32 },
                    cid: client_id,
                    mtu,
                    debug: false,
                    os: "MAC".to_string(),
                }),
                ..Default::default()
            },
        });

        let mut buf = vec![];
        msg.serialize(&mut buf)?;

        let (amount_sent, abort) =
            Self::retrying_send_to(&socket, &buf[..], "255.255.255.255:2018")?;
        assert_eq!(amount_sent, buf.len());

        let camera_address;
        let camera_id;
        loop {
            if (OffsetDateTime::now_utc() - start_time) <= timeout {
                let mut buf = vec![0; mtu as usize];
                if let Ok((bytes_read, socket_addr)) = socket.recv_from(&mut buf) {
                    match BcUdp::deserialize(&buf[0..bytes_read]) {
                        Ok(BcUdp::Discovery(UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    d2c_c_r: Some(D2cCr { did, cid, .. }),
                                    ..
                                },
                        })) if cid == client_id => {
                            abort.abort();
                            camera_address = socket_addr;
                            camera_id = did;
                            break;
                        }
                        Ok(smt) => debug!("Udp Discovery got this unexpected BcUdp {:?}", smt),
                        Err(_) => debug!(
                            "Udp Discovery got this unexpected binary: {:?}",
                            &buf[0..bytes_read]
                        ),
                    }
                }
            } else {
                abort.abort();
                return Err(Error::Timeout);
            }
        }

        // Ensure we didn't forget to abort the sending
        abort.abort();
        // Actual connection time!
        socket.connect(camera_address)?;

        Ok(DiscoveryResult {
            socket,
            address: camera_address,
            client_id,
            camera_id,
            mtu,
        })
    }

    pub fn new(uid: &str, timeout: Duration) -> Result<Self> {
        let discovery_result = Self::discover_from_uuid(uid, timeout)?;
        info!("UID {:?} found at {:?}", uid, discovery_result.address);

        let me = UdpSource {
            socket: discovery_result.socket,
            address: discovery_result.address,
            client_id: discovery_result.client_id,
            camera_id: discovery_result.camera_id,
            mtu: discovery_result.mtu,
            incoming: Default::default(),
            outgoing: Default::default(),
            aborter: AbortHandle::new(),
            next_to_consume: Arc::new(AtomicU32::new(0)),
            next_send_message: Arc::new(AtomicU32::new(0)),
            read_buffer: Default::default(),
            write_buffer: Default::default(),
        };

        me.start_polling()?;

        Ok(me)
    }

    pub fn try_clone(&self) -> IoResult<Self> {
        Ok(Self {
            socket: self.socket.try_clone()?,
            address: self.address,
            client_id: self.client_id,
            camera_id: self.camera_id,
            mtu: self.mtu,
            incoming: self.incoming.clone(),
            outgoing: self.outgoing.clone(),
            aborter: self.aborter.clone(),
            next_to_consume: self.next_to_consume.clone(),
            next_send_message: self.next_send_message.clone(),
            // These are not shared mutable so they don't pollute each other buffer
            read_buffer: self.read_buffer.clone(),
            write_buffer: self.write_buffer.clone(),
        })
    }

    fn start_polling(&self) -> IoResult<()> {
        self.start_polling_recv()?;
        self.start_polling_send()?;
        Ok(())
    }

    fn handle_ack(
        next_to_consume: u32,
        camera_id: u32,
        incoming: &mut HashMap<u32, QueuedMessage>,
        socket: &UdpSocket,
    ) {
        // Send an acknoledge.
        // we want the next packet that's missing after the
        // last consumed packet.
        let mut next_packet = next_to_consume;
        loop {
            if incoming.get(&next_packet).is_none() {
                break;
            } else {
                next_packet += 1;
            }
        }
        if next_packet > 0 {
            let last_received_contigous_packet = next_packet - 1;
            let ack_msg = BcUdp::Ack(UdpAck {
                connection_id: camera_id,
                packet_id: last_received_contigous_packet,
            });
            debug!("Re ack packet: {}", last_received_contigous_packet);
            let mut ack_buf = vec![];
            if ack_msg.serialize(&mut ack_buf).is_ok() {
                if let Ok(amount_sent) = socket.send(&ack_buf[..]) {
                    assert_eq!(amount_sent, ack_buf.len());
                } else {
                    error!("Unable to send acknoledgement");
                }
            } else {
                error!("Unable to serialize acknoledgement");
            }
        }
    }

    fn start_polling_recv(&self) -> IoResult<()> {
        let thread_aborter = self.aborter.clone();
        let thread_incoming = self.incoming.clone();
        let thread_outgoing = self.outgoing.clone();
        let thread_socket = self.socket.try_clone()?;
        let thread_mtu = self.mtu;
        let thread_client_id = self.client_id;
        let thread_camera_id = self.camera_id;
        let thread_next_consumed = self.next_to_consume.clone();

        std::thread::spawn(move || {
            while !thread_aborter.is_aborted() {
                debug!("Poll UDP Read");
                // Reciving
                let mut read_buf = vec![0_u8; thread_mtu as usize];
                if let Ok(bytes_read) = thread_socket.recv(&mut read_buf[..]) {
                    match BcUdp::deserialize(&read_buf[0..bytes_read]) {
                        Ok(BcUdp::Discovery(packet)) => {
                            warn!("Got unexpected discovery packet: {:?}", packet)
                        }
                        Ok(BcUdp::Ack(UdpAck {
                            connection_id: cid,
                            packet_id,
                        })) if cid == thread_client_id => {
                            // Camera got our message remove older ones from the send queue
                            debug!("Got acknoledgment of {}", packet_id);
                            thread_outgoing
                                .lock()
                                .unwrap()
                                .retain(|&k, _| k > packet_id);
                            // Queue for resend now
                            thread_outgoing
                                .lock()
                                .unwrap()
                                .iter_mut()
                                .for_each(|(_, v)| v.time_last_tried = None);

                            Self::handle_ack(
                                thread_next_consumed.load(Ordering::Relaxed),
                                thread_camera_id,
                                &mut *thread_incoming.lock().unwrap(),
                                &thread_socket,
                            );
                        }
                        Ok(BcUdp::Data(UdpData {
                            connection_id: cid,
                            packet_id,
                            payload,
                        })) if cid == thread_client_id => {
                            // Got some data add it to our buffer
                            let consumed_packet = thread_next_consumed.load(Ordering::Relaxed);
                            if packet_id >= consumed_packet {
                                debug!("Reciving UDP data packet with ID: {}", packet_id);
                                thread_incoming.lock().unwrap().insert(
                                    packet_id,
                                    QueuedMessage {
                                        buf: payload,
                                        time_last_tried: None,
                                    },
                                );

                                Self::handle_ack(
                                    thread_next_consumed.load(Ordering::Relaxed),
                                    thread_camera_id,
                                    &mut *thread_incoming.lock().unwrap(),
                                    &thread_socket,
                                );
                            }
                        }
                        Ok(bcudp) => warn!("Unexpected bcudp received {:?}", bcudp),
                        Err(e) => error!("Unable to poll from udp socket {:?}", e),
                    }
                }
            }
        });

        Ok(())
    }

    fn start_polling_send(&self) -> IoResult<()> {
        let thread_aborter = self.aborter.clone();
        let thread_outgoing = self.outgoing.clone();
        let thread_socket = self.socket.try_clone()?;

        std::thread::spawn(move || {
            while !thread_aborter.is_aborted() {
                // debug!("Poll UDP Send");
                let now = OffsetDateTime::now_utc();
                for (packet_id, message) in thread_outgoing.lock().unwrap().iter_mut() {
                    let mut should_send = false;
                    if let Some(&time_last_tried) = message.time_last_tried.as_ref() {
                        if (now - time_last_tried) >= WAIT_TIME {
                            should_send = true;
                        }
                    } else {
                        should_send = true;
                    }
                    if should_send {
                        debug!(
                            "Sending message ID {} with payload len {}",
                            packet_id,
                            message.buf.len()
                        );
                        message.time_last_tried = Some(now);
                        if thread_socket.send(&message.buf[..]).is_err() {
                            warn!("Failed to send message on udp");
                        }
                    }
                }
            }
        });

        Ok(())
    }

    fn stop_polling(&self) {
        self.aborter.abort();
    }
}

// Ensuring polling stops
impl Drop for UdpSource {
    fn drop(&mut self) {
        self.stop_polling();
    }
}

impl Read for UdpSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
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
            let start_time = OffsetDateTime::now_utc();
            while (start_time - OffsetDateTime::now_utc()) < RX_TIMEOUT {
                if let Some(msg) = self
                    .incoming
                    .lock()
                    .unwrap()
                    .remove(&self.next_to_consume.load(Ordering::Relaxed))
                {
                    self.next_to_consume.fetch_add(1, Ordering::Relaxed);
                    self.read_buffer.buffer.extend(msg.buf);
                    break;
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
        self.write_buffer.buffer.extend(buf.to_vec());
        if self.write_buffer.buffer.len() > self.mtu as usize - UDPDATA_HEADER_SIZE {
            let _ = self.flush();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        for chunk in self
            .write_buffer
            .buffer
            .chunks(self.mtu as usize - UDPDATA_HEADER_SIZE)
        {
            let packet_id = self.next_send_message.fetch_add(1, Ordering::Relaxed);
            let msg = BcUdp::Data(UdpData {
                connection_id: self.camera_id,
                packet_id,
                payload: chunk.to_vec(),
            });
            let mut buf = vec![];
            // If this errors it is unrecoverable
            //
            // It really shouldn't be able to Err though
            msg.serialize(&mut buf)
                .expect("Failed to serialize UDP Data");
            debug!("Writing");
            self.outgoing.lock().unwrap().insert(
                packet_id,
                QueuedMessage {
                    time_last_tried: None,
                    buf,
                },
            );
        }
        self.write_buffer.buffer.clear();
        Ok(())
    }
}

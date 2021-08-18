use super::{BcSubscription, Error, Result};
use crate::bc;
use crate::bc::model::*;
use crate::bcudp;
use crate::bcudp::{model::*, xml::*};
use crate::RX_TIMEOUT;
use lazy_static::lazy_static;
use log::*;
use rand::{seq::SliceRandom, thread_rng, Rng};
use socket2::{Domain, Socket, Type};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::error::Error as StdErr; // Just need the traits
use std::io::{BufRead, Error as IoError, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
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

// TODO: Path MTU discovery
const MTU: u32 = 1350;

lazy_static! {
    static ref P2P_RELAY_HOSTNAMES: [&'static str; 10] = [
        "p2p.reolink.com",
        "p2p1.reolink.com",
        "p2p2.reolink.com",
        "p2p3.reolink.com",
        "p2p14.reolink.com",
        "p2p15.reolink.com",
        "p2p6.reolink.com",
        "p2p7.reolink.com",
        "p2p8.reolink.com",
        "p2p9.reolink.com",
    ];
}

pub struct UdpSource {
    socket: UdpSocket,
    address: SocketAddr,
    client_id: i32,
    camera_id: i32,
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
    client_id: i32,
    camera_id: i32,
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
        let addr = addr.to_socket_addrs()?.next().ok_or_else(|| {
            IoError::new(std::io::ErrorKind::NotFound, Error::ConnectionUnavaliable)
        })?;
        Self::retrying_send_to_multi(socket, buf, &[addr])
    }

    fn retrying_send_to_multi<T: ToSocketAddrs>(
        socket: &UdpSocket,
        buf: &[u8],
        addrs: &[T],
    ) -> IoResult<(usize, AbortHandle)> {
        let handle = AbortHandle::new();

        let addrs: Vec<SocketAddr> = addrs
            .iter()
            .map(|a| {
                a.to_socket_addrs()
                    .ok()
                    .and_then(|mut a_iter| a_iter.next())
            })
            .flatten()
            .collect();

        let mut bytes_send = 0;
        for addr in addrs.iter() {
            bytes_send = socket.send_to(buf, &addr)?;
        }

        let thread_buffer = buf.to_vec();
        let thread_handle = handle.clone();
        let thread_socket = socket.try_clone()?;
        let thread_addr: Vec<SocketAddr> = addrs;

        const MAX_RETRIES: usize = 10;

        std::thread::spawn(move || {
            std::thread::sleep(WAIT_TIME);
            for _ in 0..MAX_RETRIES {
                if thread_handle.is_aborted() {
                    break;
                }
                for addr in thread_addr.iter() {
                    let _ = thread_socket.send_to(&thread_buffer[..], &addr);
                }
                std::thread::sleep(WAIT_TIME);
            }
        });

        Ok((bytes_send, handle))
    }

    fn discover_from_uuid_local(
        uid: &str,
        socket: &UdpSocket,
        timeout: Duration,
    ) -> Result<DiscoveryResult> {
        let mut rng = thread_rng();
        // If tid is too large it will overflow during encrypt so we just use a random u8
        let tid: u32 = (rng.gen::<u8>()) as u32;
        let client_id: i32 = rng.gen();
        let local_addr = socket.local_addr()?;
        let port = local_addr.port();

        let mtu = MTU;

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

        let (amount_sent, abort) = Self::retrying_send_to_multi(
            socket,
            &buf[..],
            &["255.255.255.255:2015", "255.255.255.255:2018"],
        )?;
        assert_eq!(amount_sent, buf.len());

        let start_time = OffsetDateTime::now_utc();
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
            socket: socket.try_clone()?,
            address: camera_address,
            client_id,
            camera_id,
            mtu,
        })
    }

    fn get_register(
        uid: &str,
        socket: &UdpSocket,
        timeout: Duration,
        tid: u32,
        // client_id: u32,
    ) -> Result<M2cQr> {
        // let local_addr = socket.local_addr()?;
        // let local_port = local_addr.port();

        let mtu = MTU;

        for p2p_relay in P2P_RELAY_HOSTNAMES.iter() {
            let msg = BcUdp::Discovery(UdpDiscovery {
                tid,
                payload: UdpXml {
                    c2m_q: Some(C2mQ {
                        uid: uid.to_string(),
                        os: "MAC".to_string(),
                    }),
                    ..Default::default()
                },
            });

            let mut buf = vec![];
            msg.serialize(&mut buf)?;

            let (amount_sent, abort) =
                Self::retrying_send_to(socket, &buf[..], format!("{}:9999", p2p_relay))?;
            assert_eq!(amount_sent, buf.len());

            let start_time = OffsetDateTime::now_utc();
            loop {
                if (OffsetDateTime::now_utc() - start_time) <= timeout {
                    let mut buf = vec![0; mtu as usize];
                    if let Ok((bytes_read, _)) = socket.recv_from(&mut buf) {
                        match BcUdp::deserialize(&buf[0..bytes_read]) {
                            Ok(BcUdp::Discovery(UdpDiscovery {
                                tid: _,
                                payload:
                                    UdpXml {
                                        m2c_q_r: Some(m2c_q_r),
                                        ..
                                    },
                            })) => {
                                abort.abort();
                                return Ok(m2c_q_r);
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
                    // This p2p server didn't have the data we need try another
                    continue;
                }
            }
        }
        Err(Error::ConnectionUnavaliable)
    }

    fn discover_from_uuid_remote(
        uid: &str,
        socket: &UdpSocket,
        timeout: Duration,
    ) -> Result<DiscoveryResult> {
        let local_addr = socket.local_addr()?;
        let local_port = local_addr.port();

        let mut rng = thread_rng();
        // If tid is too large it will overflow during encrypt so we just use a random u8
        let tid: u32 = (rng.gen::<u8>()) as u32;
        let mtu = MTU;
        let client_id: i32 = rng.gen();

        let m2c_q_r = Self::get_register(uid, socket, timeout, tid)?;

        let register_address = m2c_q_r.reg;
        let relay_address = m2c_q_r.relay;
        let log_address = m2c_q_r.log;
        let device_address = m2c_q_r.t;

        debug!("Register address found: {:?}", register_address);
        debug!("Registering this address: {:?}", local_addr);

        let local_family = if local_addr.ip().is_ipv4() { 4 } else { 6 };
        let msg = BcUdp::Discovery(UdpDiscovery {
            tid,
            payload: UdpXml {
                c2r_c: Some(C2rC {
                    uid: uid.to_string(),
                    cli: IpPort {
                        ip: local_addr.ip().to_string(),
                        port: local_port,
                    },
                    relay: relay_address,
                    cid: client_id,
                    family: local_family,
                    debug: false,
                    os: "MAC".to_string(),
                }),
                ..Default::default()
            },
        });

        let mut buf = vec![];
        msg.serialize(&mut buf)?;

        let (amount_sent, abort) = Self::retrying_send_to(
            socket,
            &buf[..],
            format!("{}:{}", register_address.ip, register_address.port),
        )?;
        assert_eq!(amount_sent, buf.len());

        let device_sid;
        let dev_loc;
        let start_time = OffsetDateTime::now_utc();
        loop {
            if (OffsetDateTime::now_utc() - start_time) <= timeout {
                let mut buf = vec![0; mtu as usize];
                if let Ok((bytes_read, _)) = socket.recv_from(&mut buf) {
                    match BcUdp::deserialize(&buf[0..bytes_read]) {
                        // Got camera data from register
                        Ok(BcUdp::Discovery(UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    r2c_t: Some(R2cT { dev, cid, sid, .. }),
                                    ..
                                },
                        })) if cid == client_id => {
                            abort.abort();
                            // Make a local request to the camera with the dev info
                            device_sid = sid;
                            dev_loc = dev;
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

        debug!("Register revealed address as SID: {:?}", device_sid);
        debug!("Register revealed address as IP: {:?}", dev_loc);

        // ensure abort
        abort.abort();

        let msg = BcUdp::Discovery(UdpDiscovery {
            tid,
            payload: UdpXml {
                c2d_t: Some(C2dT {
                    sid: device_sid,
                    cid: client_id,
                    mtu,
                    conn: "local".to_string(),
                }),
                ..Default::default()
            },
        });

        let mut buf = vec![];
        msg.serialize(&mut buf)?;

        let (amount_sent, abort) =
            Self::retrying_send_to(socket, &buf[..], format!("{}:{}", dev_loc.ip, dev_loc.port))?;
        assert_eq!(amount_sent, buf.len());

        let device_id;
        let start_time = OffsetDateTime::now_utc();
        loop {
            if (OffsetDateTime::now_utc() - start_time) <= timeout {
                let mut buf = vec![0; mtu as usize];
                if let Ok((bytes_read, _)) = socket.recv_from(&mut buf) {
                    match BcUdp::deserialize(&buf[0..bytes_read]) {
                        // Got camera data from camera
                        Ok(BcUdp::Discovery(UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    d2c_t: Some(D2cT { cid, did, .. }),
                                    ..
                                },
                        })) if cid == client_id => {
                            device_id = did;
                            break;
                        }
                        // Got camera data from camera CFM
                        Ok(BcUdp::Discovery(UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    d2c_cfm: Some(D2cCfm { cid, did, .. }),
                                    ..
                                },
                        })) if cid == client_id => {
                            device_id = did;
                            break;
                        }
                        // Got camera data from camera disc
                        Ok(BcUdp::Discovery(UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    d2c_disc: Some(D2cDisc { cid, did, .. }),
                                    ..
                                },
                        })) if cid == client_id => {
                            device_id = did;
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

        debug!("Got device ID as: {:?}", device_id);

        // Ensure aborted
        abort.abort();

        // Announce to the log that we will connect locally
        let msg = BcUdp::Discovery(UdpDiscovery {
            tid,
            payload: UdpXml {
                c2r_cfm: Some(C2rCfm {
                    sid: device_sid,
                    cid: client_id,
                    did: device_id,
                    conn: "local".to_string(),
                    rsp: 0,
                }),
                ..Default::default()
            },
        });

        let mut buf = vec![];
        msg.serialize(&mut buf)?;

        // Just let this retry to max limit as we don't get a reply
        let (amount_sent, _) = Self::retrying_send_to(
            socket,
            &buf[..],
            format!("{}:{}", log_address.ip, log_address.port),
        )?;
        assert_eq!(amount_sent, buf.len());

        // Announce a map type connection (I think this means we could connect remotely)
        let msg = BcUdp::Discovery(UdpDiscovery {
            tid,
            payload: UdpXml {
                c2d_t: Some(C2dT {
                    sid: device_sid,
                    cid: client_id,
                    mtu,
                    conn: "map".to_string(),
                }),
                ..Default::default()
            },
        });

        let mut buf = vec![];
        msg.serialize(&mut buf)?;

        // Just let this retry to max limit as we don't get a reply
        let (amount_sent, _) = Self::retrying_send_to(
            socket,
            &buf[..],
            format!("{}:{}", device_address.ip, device_address.port),
        )?;
        assert_eq!(amount_sent, buf.len());

        let camera_address: SocketAddr = format!("{}:{}", device_address.ip, device_address.port)
            .to_socket_addrs()?
            .next()
            .ok_or(Error::ConnectionUnavaliable)?;

        Ok(DiscoveryResult {
            socket: socket.try_clone()?,
            address: camera_address,
            client_id,
            camera_id: device_id,
            mtu,
        })
    }

    fn discover_from_uuid(
        uid: &str,
        timeout: Duration,
        allow_remote: bool,
    ) -> Result<DiscoveryResult> {
        let socket = Self::get_socket(timeout)?;
        match Self::discover_from_uuid_local(uid, &socket, timeout) {
            Err(Error::Timeout) if allow_remote => {
                Self::discover_from_uuid_remote(uid, &socket, timeout)
            }
            Ok(result) => Ok(result),
            Err(e) => Err(e),
        }
    }

    /// It is possible to contact the reolink servers using udp to
    /// learn the ip and port of the camera. By default I have turned
    /// this feature off with no way to enable it from the usual
    /// command line interface or config.
    ///
    /// However it may prove neccesary one day to use it so it is here in the
    /// library and other programs may want to use it.
    pub fn new_allow_remote(uid: &str, timeout: Duration) -> Result<Self> {
        Self::new_with_remote(uid, timeout, true)
    }

    pub fn new(uid: &str, timeout: Duration) -> Result<Self> {
        Self::new_with_remote(uid, timeout, false)
    }

    fn new_with_remote(uid: &str, timeout: Duration, allow_remote: bool) -> Result<Self> {
        let discovery_result = Self::discover_from_uuid(uid, timeout, allow_remote)?;
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
        camera_id: i32,
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

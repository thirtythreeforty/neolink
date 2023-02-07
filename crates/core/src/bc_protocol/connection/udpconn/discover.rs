use super::{aborthandle::AbortHandle, Error, Result, MTU, P2P_RELAY_HOSTNAMES, WAIT_TIME};
use crate::bcudp::{model::*, xml::*};
use local_ip_address::local_ip;
use log::*;
use rand::{thread_rng, Rng};
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;
use time::OffsetDateTime;

pub struct UdpDiscover {
    pub client_id: i32,
    pub camera_id: i32,
    pub mtu: u32,
    pub address: SocketAddr,
}

impl UdpDiscover {
    // Sends data on a UDP socket until aborted or max retries reached
    fn retrying_send_to<T: ToSocketAddrs>(
        socket: &UdpSocket,
        buf: &[u8],
        addr: T,
    ) -> IoResult<(usize, AbortHandle)> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| IoError::new(ErrorKind::NotFound, Error::ConnectionUnavaliable))?;
        Self::retrying_send_to_multi(socket, buf, &[addr])
    }

    // Sends data to multiple destinations using a UDP socket
    // until aborted or max retries reached
    fn retrying_send_to_multi<T: ToSocketAddrs>(
        socket: &UdpSocket,
        buf: &[u8],
        addrs: &[T],
    ) -> IoResult<(usize, AbortHandle)> {
        let handle = AbortHandle::new();

        let addrs: Vec<SocketAddr> = addrs
            .iter()
            .flat_map(|a| {
                a.to_socket_addrs()
                    .ok()
                    .and_then(|mut a_iter| a_iter.next())
            })
            .collect();

        let mut bytes_send = 0;
        for addr in addrs.iter() {
            bytes_send = socket.send_to(buf, addr)?;
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
                    let _ = thread_socket.send_to(&thread_buffer[..], addr);
                }
                std::thread::sleep(WAIT_TIME);
            }
        });

        Ok((bytes_send, handle))
    }

    // Performs local discovery
    //
    // This involves broadcasting a C2dC
    // Bc Discovery packet to ports 2015 and 2018
    // and awaiting a D2cCr reply
    fn discover_from_uuid_local(socket: &UdpSocket, uid: &str, timeout: Duration) -> Result<Self> {
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

        let mut broadcasts = vec![Ipv4Addr::BROADCAST];
        for iface in get_if_addrs::get_if_addrs()?.iter() {
            if let get_if_addrs::IfAddr::V4(ifacev4) = &iface.addr {
                if let Some(broadcast) = ifacev4.broadcast.as_ref() {
                    broadcasts.push(*broadcast);
                }
            }
        }
        let ports: [u16; 2] = [2015, 2018];
        let destinations: Vec<(Ipv4Addr, u16)> = broadcasts
            .iter()
            .flat_map(|&addr| {
                ports
                    .iter()
                    .map(|&port| (addr, port))
                    .collect::<Vec<(Ipv4Addr, u16)>>()
            })
            .collect();
        debug!("Broadcasting to: {:?}", destinations);

        let (amount_sent, abort) = Self::retrying_send_to_multi(socket, &buf[..], &destinations)?;
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

        Ok(UdpDiscover {
            address: camera_address,
            client_id,
            camera_id,
            mtu,
        })
    }

    // This function will contact the p2p relay servers
    //
    // It will ask each of the servers for details on a specific UID
    //
    // On success it returns the M2cQr that the p2p relay
    // server has about the UID
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
            debug!("Trying register: {}", p2p_relay);
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
                            })) if m2c_q_r.reg.port == 0 || m2c_q_r.reg.ip.is_empty() => {
                                // This register is empty
                                // Sometimes a registery will return a M2cQr but it's port and addr
                                // will be 0
                                // In this case we ignore this server and contact another
                                abort.abort();
                                break;
                            }
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
                    break;
                }
            }
        }
        debug!("All registers tried");
        Err(Error::ConnectionUnavaliable)
    }

    // This will start remote discovery against the reolink p2p servers
    //
    // It has the following stages
    // - Contact the p2p register: This will get the register, relay and log server ip addresses
    // - Register the client ip address to the register (This can probably be used for NAT holepunching if the camera
    //    tries to connect to us using this data)
    // - The register will return the camera ip/port and SID
    // - Connect to the camera using register's provided ip/port and negotiate a cid and did
    // - Send a message to the reolink log server that we will connect locally
    // - Send a message to the camera that we could connect remotely
    fn discover_from_uuid_remote(
        socket: &UdpSocket,
        uid: &str,
        timeout: Duration,
    ) -> Result<UdpDiscover> {
        let local_addr = socket.local_addr()?;
        let local_port = local_addr.port();

        let mut rng = thread_rng();
        // If tid is too large it will overflow during encrypt so we just use a random u8
        let tid: u32 = (rng.gen::<u8>()) as u32;
        let mtu = MTU;
        let client_id: i32 = rng.gen();

        let m2c_q_r = Self::get_register(uid, socket, timeout, tid)?;

        debug!("Got this information from the register: {:?}", m2c_q_r);

        let register_address = m2c_q_r.reg;
        let relay_address = m2c_q_r.relay;
        let log_address = m2c_q_r.log;
        // let device_address = m2c_q_r.t;

        let default_local_address = local_ip().expect("There to be a local ip");
        debug!("Register address found: {:?}", register_address);
        debug!("Registering this address: {:?}", default_local_address);

        let local_family = if local_addr.ip().is_ipv4() { 4 } else { 6 };
        let msg = BcUdp::Discovery(UdpDiscovery {
            tid,
            payload: UdpXml {
                c2r_c: Some(C2rC {
                    uid: uid.to_string(),
                    cli: IpPort {
                        ip: default_local_address.to_string(),
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
                        })) if cid == client_id && !dev.ip.is_empty() && dev.port > 0 => {
                            abort.abort();
                            // Make a local request to the camera with the dev info
                            device_sid = sid;
                            dev_loc = dev;
                            break;
                        }
                        Ok(BcUdp::Discovery(UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    r2c_t: Some(R2cT { cid, .. }),
                                    ..
                                },
                        })) if cid == client_id => {
                            // Got a reply but the ip/port was empty please wait
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
        let (amount_sent, _) =
            Self::retrying_send_to(socket, &buf[..], format!("{}:{}", dev_loc.ip, dev_loc.port))?;
        assert_eq!(amount_sent, buf.len());

        let camera_address: SocketAddr = format!("{}:{}", dev_loc.ip, dev_loc.port)
            .to_socket_addrs()?
            .next()
            .ok_or(Error::ConnectionUnavaliable)?;

        socket.connect(camera_address)?;

        Ok(UdpDiscover {
            address: camera_address,
            client_id,
            camera_id: device_id,
            mtu,
        })
    }

    pub fn discover_from_uuid(
        socket: &UdpSocket,
        uid: &str,
        timeout: Duration,
        allow_remote: bool,
    ) -> Result<Self> {
        match Self::discover_from_uuid_local(socket, uid, timeout) {
            Err(Error::Timeout) if allow_remote => {
                info!("Trying remote discovery against reolink servers");
                Self::discover_from_uuid_remote(socket, uid, timeout)
            }
            Ok(result) => Ok(result),
            Err(e) => Err(e),
        }
    }

    pub fn send_client_disconnect(&self, socket: &UdpSocket) {
        if let Ok(addr) = socket.peer_addr() {
            let mut rng = thread_rng();
            let tid: u32 = (rng.gen::<u8>()) as u32;
            let bcudp_msg = BcUdp::Discovery(UdpDiscovery {
                tid,
                payload: UdpXml {
                    c2d_disc: Some(C2dDisc {
                        cid: self.client_id,
                        did: self.camera_id,
                    }),
                    ..Default::default()
                },
            });

            let mut buf = vec![];
            let buf = bcudp_msg
                .serialize(&mut buf)
                .expect("Unable to serliaze udp disconnect");
            let _ = Self::retrying_send_to(socket, buf, addr);
        }
    }
}

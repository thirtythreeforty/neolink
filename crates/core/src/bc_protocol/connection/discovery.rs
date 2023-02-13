//! This module handles discovery
//!
//! Given a UID find the associated IP
//!
use super::DiscoveryResult;
use crate::bcudp::codex::BcUdpCodex;
use crate::bcudp::model::*;
use crate::bcudp::xml::*;
use crate::{Error, Result};
use futures::{
    sink::SinkExt,
    stream::{FuturesUnordered, SplitSink, StreamExt},
};
use lazy_static::lazy_static;
use local_ip_address::local_ip;
use log::*;
use rand::{seq::SliceRandom, thread_rng, Rng};
use std::collections::{btree_map::Entry, BTreeMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use tokio::{
    net::UdpSocket,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Mutex, RwLock,
    },
    task::JoinSet,
    time::{interval, timeout, Duration},
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::udp::UdpFramed;

pub(crate) struct Discovery {}

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

type Subscriber = Arc<RwLock<BTreeMap<u32, Sender<Result<(UdpDiscovery, SocketAddr)>>>>>;
type ArcFramedSocket = UdpFramed<BcUdpCodex, Arc<UdpSocket>>;
pub(crate) struct Discoverer {
    socket: Arc<UdpSocket>,
    handle: JoinSet<()>,
    writer: Mutex<SplitSink<ArcFramedSocket, (BcUdp, SocketAddr)>>,
    subsribers: Subscriber,
    local_addr: SocketAddr,
}

impl Discoverer {
    async fn new() -> Result<Discoverer> {
        let socket = Arc::new(connect().await?);
        let local_addr = socket.local_addr()?;
        let inner: ArcFramedSocket = UdpFramed::new(socket.clone(), BcUdpCodex::new());

        let (writer, mut reader) = inner.split();
        let mut set = JoinSet::new();
        let subsribers: Subscriber = Default::default();

        let thread_subscriber = subsribers.clone();
        set.spawn(async move {
            loop {
                match reader.next().await {
                    Some(Ok((BcUdp::Discovery(bcudp), addr))) => {
                        let tid = bcudp.tid;
                        let mut needs_removal = false;
                        if let Some(sender) = thread_subscriber.read().await.get(&tid) {
                            if sender.send(Ok((bcudp, addr))).await.is_err() {
                                needs_removal = true;
                            }
                        } else {
                            debug!("Udp Discovery got this unexpected BcUdp {:?}", bcudp);
                        }
                        if needs_removal {
                            thread_subscriber.write().await.remove(&tid);
                        }
                    }
                    Some(Ok(bcudp)) => {
                        // Only discovery packets should be possible atm
                        error!("Got non Discovery during discovery: {:?}", bcudp);
                        unreachable!()
                    }
                    Some(Err(e)) => {
                        let mut locked_sub = thread_subscriber.write().await;
                        for (_, sub) in locked_sub.iter() {
                            let _ = sub.send(Err(e.clone())).await;
                        }
                        locked_sub.clear();
                        break;
                    }
                    None => break,
                }
            }
        });

        Ok(Discoverer {
            socket,
            handle: set,
            writer: Mutex::new(writer),
            subsribers,
            local_addr,
        })
    }

    async fn into_socket(mut self) -> UdpSocket {
        let socket = self.socket.clone();
        self.handle.shutdown().await;
        drop(self);
        Arc::try_unwrap(socket).expect("Should not be shared at this point")
    }

    async fn subscribe(&self, tid: u32) -> Result<Receiver<Result<(UdpDiscovery, SocketAddr)>>> {
        let mut subs = self.subsribers.write().await;
        match subs.entry(tid) {
            Entry::Vacant(vacant) => {
                let (tx, rx) = channel(10);
                vacant.insert(tx);
                Ok(rx)
            }
            Entry::Occupied(mut occ) => {
                if occ.get().is_closed() {
                    let (tx, rx) = channel(10);
                    occ.insert(tx);
                    Ok(rx)
                } else {
                    Err(Error::SimultaneousSubscription {
                        msg_num: (tid as u16),
                    })
                }
            }
        }
    }

    /// Subsciber others is for messages that we did not initiate and are therefore
    /// using an unknown tid
    /// In this case we subscribe to tid 0
    async fn handle_incoming<T, F>(&self, map: F) -> Result<T>
    where
        F: Fn(UdpDiscovery, SocketAddr) -> Option<T>,
    {
        let mut reply = ReceiverStream::new(self.subscribe(0).await?);
        loop {
            let (reply, addr) = reply.next().await.ok_or(Error::ConnectionUnavaliable)??;
            if let Some(result) = map(reply, addr) {
                return Ok(result);
            }
        }
    }

    async fn send(&self, disc: BcUdp, addr: SocketAddr) -> Result<()> {
        self.writer.lock().await.send((disc, addr)).await
    }

    fn local_addr(&self) -> &SocketAddr {
        &self.local_addr
    }

    async fn retry_send_multi<F, T>(
        &self,
        disc: UdpDiscovery,
        dests: &[SocketAddr],
        map: F,
    ) -> Result<T>
    where
        F: Fn(UdpDiscovery, SocketAddr) -> Option<T>,
    {
        let mut set = FuturesUnordered::new();
        for dest in dests.iter() {
            set.push(self.retry_send(disc.clone(), *dest, &map));
        }

        // Get what ever completes first
        while let Some(result) = set.next().await {
            if result.is_ok() {
                return result;
            }
        }
        Err(Error::DiscoveryTimeout)
    }

    async fn retry_send<F, T>(&self, mut disc: UdpDiscovery, dest: SocketAddr, map: F) -> Result<T>
    where
        F: Fn(UdpDiscovery, SocketAddr) -> Option<T>,
    {
        let target_tid = if disc.tid == 0 {
            // If 0 make a random one
            let target_tid = generate_tid();
            disc.tid = target_tid;
            target_tid
        } else {
            disc.tid
        };
        let mut reply = ReceiverStream::new(self.subscribe(target_tid).await?);
        let msg = BcUdp::Discovery(disc);

        let mut inter = interval(Duration::from_millis(500));

        for _i in 0..5 {
            inter.tick().await;

            self.send(msg.clone(), dest).await?;

            if let Ok(Some(Ok((reply, addr)))) =
                timeout(Duration::from_millis(500), reply.next()).await
            {
                if let Some(result) = map(reply, addr) {
                    return Ok(result);
                }
            }
        }
        Err(Error::DiscoveryTimeout)
    }
}

impl Discovery {
    pub(crate) async fn local(uid: &str) -> Result<DiscoveryResult> {
        trace!("Local");
        let discoverer = Discoverer::new().await?;

        let client_id = generate_cid();

        let local_addr = discoverer.local_addr();
        let port = local_addr.port();

        let msg = UdpDiscovery {
            tid: generate_tid(),
            payload: UdpXml {
                c2d_c: Some(C2dC {
                    uid: uid.to_string(),
                    cli: ClientList { port: port as u32 },
                    cid: client_id,
                    mtu: MTU,
                    debug: false,
                    os: "MAC".to_string(),
                }),
                ..Default::default()
            },
        };

        trace!("Local: Sending Broadcast");
        let dests = get_broadcasts(&[2015, 2018])?;
        let (camera_address, camera_id) = discoverer
            .retry_send_multi(msg, &dests, |bc, addr| match bc {
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            d2c_c_r: Some(D2cCr { did, cid, .. }),
                            ..
                        },
                } if cid == client_id => Some((addr, did)),
                _ => None,
            })
            .await?;

        trace!("Local: Success");
        Ok(DiscoveryResult {
            socket: discoverer.into_socket().await,
            addr: camera_address,
            camera_id,
            client_id,
        })
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
    #[allow(unused)]
    pub(crate) async fn remote(uid: &str) -> Result<DiscoveryResult> {
        trace!("Remote");
        let mut discoverer = Discoverer::new().await?;

        let client_id = generate_cid();

        let local_addr = *discoverer.local_addr();
        let local_port = local_addr.port();

        trace!("Remote: Finding Register");
        let reg_packet = get_register(&discoverer, uid).await?;
        trace!("Remote: Register Found");

        let register_address = reg_packet.reg;
        let relay_address = reg_packet.relay;
        let log_address = reg_packet.log;

        let default_local_address = local_ip().expect("There to be a local ip");
        debug!("Register address found: {:?}", register_address);
        debug!("Registering this address: {:?}", default_local_address);

        trace!("Remote: Registering local IP");
        let local_family = if local_addr.ip().is_ipv4() { 4 } else { 6 };
        let msg = UdpDiscovery {
            tid: 0,
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
                    revision: None,
                }),
                ..Default::default()
            },
        };

        let register_address_ip: IpAddr = register_address.ip.parse()?;
        let (device_sid, dev_loc) = discoverer
            .retry_send_multi(
                msg,
                &[SocketAddr::new(register_address_ip, register_address.port)],
                |bc, _| match bc {
                    UdpDiscovery {
                        tid: _,
                        payload:
                            UdpXml {
                                r2c_t:
                                    Some(R2cT {
                                        dev: Some(dev),
                                        cid,
                                        sid,
                                        ..
                                    }),
                                ..
                            },
                    } if cid == client_id && !dev.ip.is_empty() && dev.port > 0 => Some((sid, dev)),
                    _ => None,
                },
            )
            .await?;

        debug!("Register revealed address as SID: {:?}", device_sid);
        debug!("Register revealed address as IP: {:?}", dev_loc);

        trace!("Remote: Aquiring device ID");
        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2d_t: Some(C2dT {
                    sid: device_sid,
                    cid: client_id,
                    mtu: MTU,
                    conn: "local".to_string(),
                }),
                ..Default::default()
            },
        };

        let mut device_addr = SocketAddr::new(dev_loc.ip.parse()?, dev_loc.port);
        let device_id = tokio::select! {
            v = discoverer
                .retry_send(
                    msg,
                    device_addr,
                    |bc, _| match bc {
                        UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    d2c_disc: Some(D2cDisc { cid, did, .. }),
                                    ..
                                },
                        } if cid == client_id => Some(did),
                        UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    d2c_t: Some(D2cT { cid, did, .. }),
                                    ..
                                },
                        } if cid == client_id => Some(did),
                        _ => None,
                    },
                ) => {v?},
            // The camera can observe the registered details above and
            // use them to establish it's own connection (hold punching)
            // This branch handles those new camera connections
            v =
                discoverer
                .handle_incoming(|bc, addr| match bc {
                        UdpDiscovery {
                            tid: _,
                            payload:
                                UdpXml {
                                    d2c_t:
                                        Some(D2cT {
                                            sid,
                                            cid,
                                            did,
                                            conn,
                                            ..
                                        }),
                                    ..
                                },
                        } if cid == client_id && sid == device_sid => {
                            Some((did, addr))
                        }
                        _ => None,
                    }) => {
                        let (result, addr) = v?;
                        device_addr = addr;
                        result
                    }
        };

        debug!("Got device ID as: {:?}", device_id);

        trace!("Remote: Declare local");
        let msg = UdpDiscovery {
            tid: 0,
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
        };

        discoverer
            .retry_send(
                msg,
                SocketAddr::new(log_address.ip.parse()?, log_address.port),
                |_, _| Some(()),
            )
            .await?;

        trace!("Remote: Declare map");
        let msg = UdpDiscovery {
            tid: generate_tid(),
            payload: UdpXml {
                c2d_t: Some(C2dT {
                    sid: device_sid,
                    cid: client_id,
                    mtu: MTU,
                    conn: "map".to_string(),
                }),
                ..Default::default()
            },
        };

        discoverer
            .retry_send(msg, device_addr, |_, _| Some(()))
            .await?;

        trace!("Remote: Success");
        Ok(DiscoveryResult {
            socket: discoverer.into_socket().await,
            addr: device_addr,
            client_id,
            camera_id: device_id,
        })
    }

    // This is similar to remote, except that a relay will be established.
    //
    // All future connections will go VIA the relink servers, this is for
    // cellular cameras that do not support local connections
    //
    #[allow(unused)]
    pub(crate) async fn relay(uid: &str) -> Result<DiscoveryResult> {
        trace!("Relay");
        let mut discoverer = Discoverer::new().await?;

        let client_id = generate_cid();

        let local_addr = *discoverer.local_addr();
        let local_port = local_addr.port();

        trace!("Relay: Finding Register");
        let reg_packet = get_register(&discoverer, uid).await?;
        trace!("Relay: Register Found");

        let register_address = reg_packet.reg;
        let relay_address = reg_packet.relay;
        let log_address = reg_packet.log;

        let register_socket = SocketAddr::new(register_address.ip.parse()?, register_address.port);

        let default_local_address = local_ip().expect("There to be a local ip");
        debug!("Register address found: {:?}", register_address);
        debug!("Registering this address: {:?}", default_local_address);

        let local_family = if local_addr.ip().is_ipv4() { 4 } else { 6 };
        let msg = UdpDiscovery {
            tid: 0,
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
                    revision: Some(3),
                }),
                ..Default::default()
            },
        };
        trace!("Relay: Declaring Local IP: {}", msg.tid);

        let (device_sid, dmap, did) = discoverer
            .retry_send(msg, register_socket, |bc, _| match bc {
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            r2c_t:
                                Some(R2cT {
                                    dmap: Some(dmap),
                                    sid,
                                    cid,
                                    ..
                                }),
                            ..
                        },
                } if !dmap.ip.is_empty() && dmap.port > 0 && cid == client_id => {
                    Some((sid, Some(dmap), None))
                }
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            r2c_c_r: Some(R2cCr { dmap, sid, .. }),
                            ..
                        },
                } if !dmap.ip.is_empty() && dmap.port > 0 => Some((sid, Some(dmap), None)),
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            d2c_t: Some(D2cT { sid, did, .. }),
                            ..
                        },
                } => Some((sid, None, Some(did))),
                _ => None,
            })
            .await?;

        debug!("Register revealed dmap address as SID: {:?}", device_sid);
        debug!("Register revealed dmap address as IP: {:?}", dmap);
        debug!("Register revealed device ID: {:?}", did);

        let mut device_addr = register_socket;
        let device_id = if let Some(dmap) = dmap {
            let msg = UdpDiscovery {
                tid: 0,
                payload: UdpXml {
                    c2d_t: Some(C2dT {
                        sid: device_sid,
                        cid: client_id,
                        mtu: MTU,
                        conn: "relay".to_string(),
                    }),
                    ..Default::default()
                },
            };
            trace!("Relay: Aquiring Device ID: {}", msg.tid);

            tokio::select! {
                v = discoverer
                .retry_send(msg, device_addr, |bc, _| match bc {
                    UdpDiscovery {
                        tid: _,
                        payload:
                            UdpXml {
                                d2c_cfm: Some(D2cCfm { cid, did, .. }),
                                ..
                            },
                    } if cid == client_id => Some(did),
                    _ => None,
                }) => {v?},
                // The camera can observe the registered details above and
                // use them to establish it's own connection (hold punching)
                // This branch handles those new camera connections
                v =
                    discoverer
                    .handle_incoming(|bc, addr| match bc {
                            UdpDiscovery {
                                tid: _,
                                payload:
                                    UdpXml {
                                        d2c_t:
                                            Some(D2cT {
                                                sid,
                                                cid,
                                                did,
                                                conn,
                                                ..
                                            }),
                                        ..
                                    },
                            } if cid == client_id && sid == device_sid => {
                                Some((did, addr))
                            }
                            _ => None,
                        }) => {
                            let (result, addr) = v?;
                            device_addr = addr;
                            result
                        }
            }
        } else {
            did.unwrap()
        };

        debug!("Got device ID as: {:?}", device_id);

        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2r_cfm: Some(C2rCfm {
                    sid: device_sid,
                    cid: client_id,
                    did: device_id,
                    rsp: 0,
                    conn: "relay".to_string(),
                }),
                ..Default::default()
            },
        };
        trace!("Relay: Declare Relay: {}", msg.tid);

        discoverer
            .retry_send(
                msg,
                SocketAddr::new(log_address.ip.parse()?, log_address.port),
                |_, _| Some(()),
            )
            .await?;

        trace!("Relay: Success");
        Ok(DiscoveryResult {
            socket: discoverer.into_socket().await,
            addr: device_addr,
            client_id,
            camera_id: device_id,
        })
    }
}

// This function will contact the p2p relay servers
//
// It will ask each of the servers for details on a specific UID
//
// On success it returns the M2cQr that the p2p relay
// server has about the UID
async fn get_register(discoverer: &Discoverer, uid: &str) -> Result<M2cQr> {
    let mut addrs = vec![];
    for p2p_relay in P2P_RELAY_HOSTNAMES.iter() {
        debug!("Trying register: {}", p2p_relay);
        addrs.append(
            &mut format!("{}:9999", p2p_relay)
                .to_socket_addrs()
                .map(|i| i.collect::<Vec<SocketAddr>>())
                .unwrap_or_else(|_| vec![]),
        );
    }
    let msg = UdpDiscovery {
        tid: 0,
        payload: UdpXml {
            c2m_q: Some(C2mQ {
                uid: uid.to_string(),
                os: "MAC".to_string(),
            }),
            ..Default::default()
        },
    };
    let (packet, addr) = discoverer
        .retry_send_multi(msg, addrs.as_slice(), |bc, addr| match bc {
            UdpDiscovery {
                tid: _,
                payload:
                    UdpXml {
                        m2c_q_r: Some(m2c_q_r),
                        ..
                    },
            } if m2c_q_r.reg.port != 0 || !m2c_q_r.reg.ip.is_empty() => Some((m2c_q_r, addr)),
            _ => None,
        })
        .await?;

    debug!(
        "Registry details found at {:?}: {}:{}",
        addr, packet.reg.ip, packet.reg.port,
    );
    Ok(packet)
}

fn get_broadcasts(ports: &[u16]) -> Result<Vec<SocketAddr>> {
    let mut broadcasts = vec![Ipv4Addr::BROADCAST];
    for iface in get_if_addrs::get_if_addrs()?.iter() {
        if let get_if_addrs::IfAddr::V4(ifacev4) = &iface.addr {
            if let Some(broadcast) = ifacev4.broadcast.as_ref() {
                broadcasts.push(*broadcast);
            }
        }
    }
    let mut destinations: Vec<(Ipv4Addr, u16)> = broadcasts
        .iter()
        .flat_map(|&addr| {
            ports
                .iter()
                .map(|&port| (addr, port))
                .collect::<Vec<(Ipv4Addr, u16)>>()
        })
        .collect();
    debug!("Broadcasting to: {:?}", destinations);
    Ok(destinations
        .drain(..)
        .map(|(addr, port)| SocketAddr::new(addr.into(), port))
        .collect())
}

fn generate_tid() -> u32 {
    let mut rng = thread_rng();
    (rng.gen::<u8>()) as u32
}

fn generate_cid() -> i32 {
    let mut rng = thread_rng();
    rng.gen()
}

async fn connect() -> Result<UdpSocket> {
    let mut ports: Vec<u16> = (53500..54000).into_iter().collect();
    {
        let mut rng = thread_rng();
        ports.shuffle(&mut rng);
    }

    let addrs: Vec<_> = ports
        .iter()
        .map(|&port| SocketAddr::from(([0, 0, 0, 0], port)))
        .collect();
    let socket = UdpSocket::bind(&addrs[..]).await?;
    socket.set_broadcast(true)?;

    Ok(socket)
}

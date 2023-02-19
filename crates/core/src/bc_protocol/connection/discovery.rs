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
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
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

#[derive(Debug, Clone)]
struct RegisterResult {
    reg: SocketAddr,
    dmap: Option<SocketAddr>,
    dev: Option<SocketAddr>,
    client_id: i32,
    sid: u32,
}

#[derive(Debug, Clone)]
struct ConnectResult {
    addr: SocketAddr,
    client_id: i32,
    camera_id: i32,
    sid: u32,
}

#[derive(Debug, Clone)]
struct UidLookupResults {
    reg: SocketAddr,
    relay: SocketAddr,
}

#[derive(Debug, Clone, Copy)]
enum ConnectionKind {
    Local,
    Relay,
}

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
                        let mut tid = bcudp.tid;
                        let mut needs_removal = false;
                        if let Some(sender) = thread_subscriber.read().await.get(&tid) {
                            if sender.send(Ok((bcudp, addr))).await.is_err() {
                                needs_removal = true;
                            }
                        } else if let Some(any_sender) = thread_subscriber.read().await.get(&0) {
                            if any_sender.send(Ok((bcudp, addr))).await.is_err() {
                                tid = 0; // To make is remove 0
                                needs_removal = true;
                            }
                        } else {
                            trace!("Udp Discovery got this unexpected BcUdp {:?}", bcudp);
                        }
                        if needs_removal {
                            thread_subscriber.write().await.remove(&tid);
                        }
                    }
                    Some(Ok(bcudp)) => {
                        // Only discovery packets should be possible atm
                        trace!("Got non Discovery during discovery: {:?}", bcudp);
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
        mut disc: UdpDiscovery,
        dests: &[SocketAddr],
        map: F,
    ) -> Result<T>
    where
        F: Fn(UdpDiscovery, SocketAddr) -> Option<T>,
    {
        disc.tid = 0; // Must be random to avoid simulatenous subscription errors
        let mut set = FuturesUnordered::new();
        for dest in dests.iter() {
            set.push(self.retry_send(disc.clone(), *dest, &map));
        }

        // Get what ever completes first
        while let Some(result) = set.next().await {
            match result {
                Ok(ret) => {
                    // info!("Future pop OK");
                    return Ok(ret);
                }
                Err(_e) => {
                    // info!("Future pop Err: {:?}", e);
                }
            }
        }
        // info!("set: empty");
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

    // This function will contact the p2p relay servers
    //
    // It will ask each of the servers for details on a specific UID
    //
    // On success it returns the M2cQr that the p2p relay
    // server has about the UID
    async fn uid_loopup(&self, uid: &str) -> Result<UidLookupResults> {
        let mut addrs = vec![];
        for p2p_relay in P2P_RELAY_HOSTNAMES.iter() {
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
        let (packet, _) = self
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

        Ok(UidLookupResults {
            reg: SocketAddr::new(packet.reg.ip.parse()?, packet.reg.port),
            relay: SocketAddr::new(packet.relay.ip.parse()?, packet.relay.port),
        })
    }

    async fn send_and_forget(&self, mut disc: UdpDiscovery, dest: SocketAddr) -> Result<()> {
        if disc.tid == 0 {
            // If 0 make a random one
            let target_tid = generate_tid();
            disc.tid = target_tid;
        }
        let mut inter = interval(Duration::from_millis(50));
        let msg = BcUdp::Discovery(disc);

        for _i in 0..5 {
            inter.tick().await;

            self.send(msg.clone(), dest).await?;
        }

        Ok(())
    }

    async fn device_initiated_connect(
        &self,
        client_id: i32,
        register_result: &RegisterResult,
    ) -> Result<ConnectResult> {
        let (addr, local_tid, local_did, conn_kind) = self
            .handle_incoming(|bc, addr| {
                trace!("bc: {:?}", bc);
                match (bc, register_result) {
                    (
                        UdpDiscovery {
                            tid,
                            payload:
                                UdpXml {
                                    d2c_t: Some(D2cT { sid, cid, did, .. }),
                                    ..
                                },
                        },
                        RegisterResult {
                            dmap: Some(register_dmap),
                            sid: register_sid,
                            ..
                        },
                    ) if cid == client_id && &sid == register_sid && &addr == register_dmap => {
                        Some((addr, tid, did, ConnectionKind::Relay))
                    }
                    (
                        UdpDiscovery {
                            tid,
                            payload:
                                UdpXml {
                                    d2c_t: Some(D2cT { sid, cid, did, .. }),
                                    ..
                                },
                        },
                        RegisterResult {
                            dev: Some(register_dev),
                            sid: register_sid,
                            ..
                        },
                    ) if cid == client_id && &sid == register_sid && &addr == register_dev => {
                        Some((addr, tid, did, ConnectionKind::Local))
                    }
                    _ => None,
                }
            })
            .await?;

        let conn = match conn_kind {
            ConnectionKind::Local => "local",
            ConnectionKind::Relay => "map",
        }
        .to_string();

        let msg = UdpDiscovery {
            tid: local_tid,
            payload: UdpXml {
                c2d_a: Some(C2dA {
                    sid: register_result.sid,
                    conn,
                    cid: client_id,
                    did: local_did,
                    mtu: MTU,
                }),
                ..Default::default()
            },
        };

        // Send and await confirm
        self.retry_send(msg, addr, |bc, _| {
            trace!("msg: {:?}", &bc);
            match bc {
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            d2c_cfm: Some(D2cCfm { sid, cid, did, .. }),
                            ..
                        },
                } if sid == register_result.sid && did == local_did && cid == client_id => Some(()),
                _ => None,
            }
        })
        .await?;

        let result = ConnectResult {
            addr,
            client_id,
            sid: register_result.sid,
            camera_id: local_did,
        };

        match conn_kind {
            ConnectionKind::Local => {
                self.confirm_map(&result, register_result.reg, result.addr)
                    .await?;
            }
            ConnectionKind::Relay => {
                self.confirm_relay(&result, register_result.reg).await?;
            }
        }

        Ok(result)
    }

    async fn client_initiated_connect(
        &self,
        register_result: &RegisterResult,
    ) -> Result<ConnectResult> {
        let tid = generate_tid();

        if let RegisterResult {
            dev: Some(dev_addr),
            ..
        } = &register_result
        {
            let msg = UdpDiscovery {
                tid,
                payload: UdpXml {
                    c2d_t: Some(C2dT {
                        sid: register_result.sid,
                        cid: register_result.client_id,
                        mtu: MTU,
                        conn: "local".to_string(),
                    }),
                    ..Default::default()
                },
            };

            let (final_addr, local_did) = self
                .retry_send(msg, *dev_addr, |bc, addr| match bc {
                    UdpDiscovery {
                        tid: _,
                        payload:
                            UdpXml {
                                d2c_cfm: Some(D2cCfm { cid, did, sid, .. }),
                                ..
                            },
                    } if cid == register_result.client_id && sid == register_result.sid => {
                        Some((addr, did))
                    }
                    _ => None,
                })
                .await?;

            let result = ConnectResult {
                addr: final_addr,
                client_id: register_result.client_id,
                sid: register_result.sid,
                camera_id: local_did,
            };

            self.confirm_map(&result, register_result.reg, *dev_addr)
                .await?;

            Ok(result)
        } else {
            Err(Error::NoDev)
        }
    }

    /// Register our local ip address with the reolink servers
    /// This will be used for the device to contact us
    async fn register_address(
        &self,
        uid: &str,
        client_id: i32,
        lookup: &UidLookupResults,
    ) -> Result<RegisterResult> {
        let tid = generate_tid();
        let local_addr = SocketAddr::new(local_ip()?, self.local_addr().port());
        let local_ip = local_addr.ip();
        let local_port = local_addr.port();
        let local_family = if local_addr.ip().is_ipv4() { 4 } else { 6 };

        let msg = UdpDiscovery {
            tid,
            payload: UdpXml {
                c2r_c: Some(C2rC {
                    uid: uid.to_string(),
                    cli: IpPort {
                        ip: local_ip.to_string(),
                        port: local_port,
                    },
                    relay: IpPort {
                        ip: lookup.relay.ip().to_string(),
                        port: lookup.relay.port(),
                    },
                    cid: client_id,
                    family: local_family,
                    debug: false,
                    os: "MAC".to_string(),
                    revision: Some(3),
                }),
                ..Default::default()
            },
        };

        // Send and await acceptance
        let (sid, dmap, dev) = self
            .retry_send(msg, lookup.reg, |bc, _| match bc {
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            r2c_t:
                                Some(R2cT {
                                    dmap: Some(dmap),
                                    sid,
                                    cid,
                                    dev: Some(dev),
                                }),
                            ..
                        },
                } if !dmap.ip.is_empty()
                    && dmap.port > 0
                    && !dev.ip.is_empty()
                    && dev.port > 0
                    && cid == client_id =>
                {
                    Some(Ok((sid, Some(dmap), Some(dev))))
                }
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            r2c_t:
                                Some(R2cT {
                                    dmap: Some(dmap),
                                    sid,
                                    cid,
                                    dev: None,
                                }),
                            ..
                        },
                } if !dmap.ip.is_empty() && dmap.port > 0 && cid == client_id => {
                    Some(Ok((sid, Some(dmap), None)))
                }
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            r2c_t:
                                Some(R2cT {
                                    dmap: None,
                                    sid,
                                    cid,
                                    dev: Some(dev),
                                }),
                            ..
                        },
                } if !dev.ip.is_empty() && dev.port > 0 && cid == client_id => {
                    Some(Ok((sid, None, Some(dev))))
                }
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            r2c_c_r: Some(R2cCr { dmap, sid, rsp, .. }),
                            ..
                        },
                } if !dmap.ip.is_empty() && dmap.port > 0 && rsp != -1 => {
                    Some(Ok((sid, Some(dmap), None)))
                }
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            r2c_c_r: Some(R2cCr { dmap, rsp, .. }),
                            ..
                        },
                } if !dmap.ip.is_empty() && dmap.port > 0 && rsp == -1 => {
                    Some(Err(Error::RegisterError))
                }
                _ => None,
            })
            .await??;

        Ok(RegisterResult {
            reg: lookup.reg,
            sid,
            client_id,
            dmap: dmap.map(|dmap| {
                SocketAddr::new(dmap.ip.parse().expect("Could not read ip addr"), dmap.port)
            }),
            dev: dev.map(|dev| {
                SocketAddr::new(dev.ip.parse().expect("Could not read ip addr"), dev.port)
            }),
        })
    }

    /// Confirm connection with relay
    /// This is sent once all data has been found
    async fn confirm_relay(&self, conn_result: &ConnectResult, reg_addr: SocketAddr) -> Result<()> {
        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2r_cfm: Some(C2rCfm {
                    sid: conn_result.sid,
                    cid: conn_result.client_id,
                    did: conn_result.camera_id,
                    rsp: 0,
                    conn: "map".to_string(),
                }),
                ..Default::default()
            },
        };

        self.send_and_forget(msg, reg_addr).await?;

        Ok(())
    }

    /// Confirm Map. This is sent if we learned the IP via the relay
    /// then connected locally. We tell the relay they we are establishing
    /// a local then tell the camera to connect to us via map
    async fn confirm_map(
        &self,
        conn_result: &ConnectResult,
        reg_addr: SocketAddr,
        dev_addr: SocketAddr,
    ) -> Result<()> {
        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2r_cfm: Some(C2rCfm {
                    sid: conn_result.sid,
                    cid: conn_result.client_id,
                    did: conn_result.camera_id,
                    conn: "local".to_string(),
                    rsp: 0,
                }),
                ..Default::default()
            },
        };

        self.send_and_forget(msg, reg_addr).await?;

        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2d_t: Some(C2dT {
                    sid: conn_result.sid,
                    cid: conn_result.client_id,
                    mtu: MTU,
                    conn: "map".to_string(),
                }),
                ..Default::default()
            },
        };

        self.send_and_forget(msg, dev_addr).await?;

        Ok(())
    }
}

impl Discovery {
    pub(crate) async fn local(uid: &str) -> Result<DiscoveryResult> {
        let discoverer = Discoverer::new().await?;

        let client_id = generate_cid();

        let local_addr = discoverer.local_addr();
        let port = local_addr.port();

        let msg = UdpDiscovery {
            tid: 0,
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
        let mut discoverer = Discoverer::new().await?;

        let client_id = generate_cid();

        let lookup = discoverer.uid_loopup(uid).await?;
        let reg_addr = lookup.reg;
        let relay_addr = lookup.relay;

        let reg_result = discoverer.register_address(uid, client_id, &lookup).await?;
        let dev_addr = reg_result.dev.ok_or(Error::NoDev)?;

        let connect_result = discoverer.client_initiated_connect(&reg_result).await?;

        Ok(DiscoveryResult {
            socket: discoverer.into_socket().await,
            addr: connect_result.addr,
            client_id,
            camera_id: connect_result.camera_id,
        })
    }

    // This is similar to remote, except that a relay (map) will be established.
    //
    // All future connections will go VIA the relink servers, this is for
    // cellular cameras that do not support local connections
    //
    // It is possible that this will also establish a local if the camera attempts that
    //
    #[allow(unused)]
    pub(crate) async fn relay(uid: &str) -> Result<DiscoveryResult> {
        let mut discoverer = Discoverer::new().await?;
        let client_id = generate_cid();

        let client_id = generate_cid();
        trace!("client_id: {}", client_id);

        let lookup = discoverer.uid_loopup(uid).await?;
        trace!("lookup: {:?}", lookup);
        let reg_addr = lookup.reg;
        let relay_addr = lookup.relay;

        let reg_result = discoverer.register_address(uid, client_id, &lookup).await?;
        trace!("reg_result: {:?}", reg_result);

        let dmap_addr = reg_result.dmap.ok_or(Error::NoDmap)?;

        let connect_result = discoverer
            .device_initiated_connect(client_id, &reg_result)
            .await?;
        trace!("connect_result: {:?}", connect_result);

        Ok(DiscoveryResult {
            socket: discoverer.into_socket().await,
            addr: connect_result.addr,
            client_id,
            camera_id: connect_result.camera_id,
        })
    }
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

/*
    # Discovery Methods

    # Register

    This is the inital query to known hosts with a known UID

    - C->R Port 9999
    ```xml
    <P2P>
    <C2M_Q>
    <uid>95270000YGAKNWKJ</uid>
    <p>MAC</p>
    </C2M_Q>
    </P2P>
    ```

    Replies with details of the camera we want to connect to

    - R->C
    ```xml
    <P2P>
        <M2C_Q_R>
            <reg>
                <ip>18.162.200.47</ip>
                <port>58200</port>
            </reg>
            <relay>
                <ip>18.162.200.47</ip>
                <port>58100</port>
            </relay>
            <log>
                <ip>18.162.200.47</ip>
                <port>57850</port>
            </log>
            <t>
                <ip>18.162.200.47</ip>
                <port>9996</port>
            </t>
            <timer/>
            <retry/>
            <mtu>1350</mtu>
            <debug>251658240</debug>
            <ac>-1700607721</ac>
            <rsp>0</rsp>
        </M2C_Q_R>
    </P2P>
    ```

    ## Thread 1: Observed during relay

    - D->C 59733
    ```xml
    <P2P>
    <D2C_T>
    <sid>495151439</sid>
    <conn>map</conn>
    <cid>254000</cid>
    <did>735</did>
    </D2C_T>
    </P2P>
    ```

    - C->D
    ```xml
    <P2P>
    <C2D_A>
    <sid>495151439</sid>
    <conn>map</conn>
    <cid>254000</cid>
    <did>735</did>
    <mtu>1350</mtu>
    </C2D_A>
    </P2P>
    ```

    - D->C
    ```xml
    <P2P>
    <D2C_CFM>
    <sid>495151439</sid>
    <conn>map</conn>
    <rsp>0</rsp>
    <cid>254000</cid>
    <did>735</did>
    <time_r>55607</time_r>
    </D2C_CFM>
    </P2P>
     ```

    ## Thread 2: Observed during relay

    - C->R: 58200
    ```xml
    <P2P>
    <C2R_C>
    <uid>95270000YGAKNWKJ</uid>
    <cli>
    <ip>10.202.133.237</ip>
    <port>12254</port>
    </cli>
    <relay>
    <ip>18.162.200.47</ip>
    <port>58100</port>
    </relay>
    <cid>254000</cid>
    <debug>251658240</debug>
    <family>4</family>
    <p>MAC</p>
    <r>3</r>
    </C2R_C>
    </P2P>
    ```

    - R->C
    ```xml
    <P2P><R2C_T><dev><ip>192.168.1.100</ip><port>57933</port></dev><dmap><ip>184.22.90.67</ip><port>57933</port></dmap><sid>495151439</sid><cid>254000</cid><rsp>0</rsp></R2C_T></P2P>
    ```

    - R->C
    ```xml
    <P2P>
        <R2C_C_R>
            <dmap>
                <ip>184.22.90.67</ip>
                <port>57933</port>
            </dmap>
            <dev>
                <ip>192.168.1.100</ip>
                <port>57933</port>
            </dev>
            <relay>
                <ip>18.162.200.47</ip>
                <port>51134</port>
            </relay>
            <relayt>
                <ip>18.162.200.47</ip>
                <port>9997</port>
            </relayt>
            <nat>NULL</nat>
            <sid>495151439</sid>
            <rsp>0</rsp>
            <ac>495151439</ac>
        </R2C_C_R>
    </P2P>
    ```

    - R->C
    ```xml
    <P2P>
        <R2C_T>
            <dev>
                <ip>192.168.1.100</ip>
                <port>57933</port>
            </dev>
            <dmap>
                <ip>184.22.90.67</ip>
                <port>57933</port>
            </dmap>
            <sid>495151439</sid>
            <cid>254000</cid>
            <rsp>0</rsp>
        </R2C_T>
    </P2P>
    ```

    - R->C Repeats later so possibly was not responded to by client
    ```xml
    <P2P>
        <R2C_C_R>
            <dmap>
                <ip>184.22.90.67</ip>
                <port>57933</port>
            </dmap>
            <dev>
                <ip>192.168.1.100</ip>
                <port>57933</port>
            </dev>
            <relay>
                <ip>18.162.200.47</ip>
                <port>51134</port>
            </relay>
            <relayt>
                <ip>18.162.200.47</ip>
                <port>9997</port>
            </relayt>
            <nat>NULL</nat>
            <sid>495151439</sid>
            <rsp>0</rsp>
            <ac>495151439</ac>
        </R2C_C_R>
    </P2P>
    ```

    - C->R
    ```xml
    <P2P>
    <C2R_CFM>
    <sid>495151439</sid>
    <conn>map</conn>
    <rsp>0</rsp>
    <cid>254000</cid>
    <did>735</did>
    </C2R_CFM>
    </P2P>
    ```

    - R->C
    ```xml
    <P2P>
        <R2C_T>
            <dev>
                <ip>192.168.1.100</ip>
                <port>57933</port>
            </dev>
            <dmap>
                <ip>184.22.90.67</ip>
                <port>57933</port>
            </dmap>
            <sid>495151439</sid>
            <cid>254000</cid>
            <rsp>0</rsp>
        </R2C_T>
    </P2P>
    ```

    # Thread 3: Observed during relay
    After connection. No response

    - C->R
    ```xml
    <P2P>
    <C2R_CFM>
    <sid>495151439</sid>
    <conn>map</conn>
    <rsp>0</rsp>
    <cid>254000</cid>
    <did>735</did>
    </C2R_CFM>
    </P2P>
    ```

*/

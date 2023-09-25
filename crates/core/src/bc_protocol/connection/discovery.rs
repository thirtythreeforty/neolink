//! This module handles discovery
//!
//! Given a UID find the associated IP
//!
use super::DiscoveryResult;
use crate::bc::model::*;
use crate::bc_protocol::{md5_string, Md5Trunc, TcpSource};
use crate::bcudp::codex::BcUdpCodex;
use crate::bcudp::model::*;
use crate::bcudp::xml::*;
use crate::{Error, Result};
use futures::{
    sink::SinkExt,
    stream::{FuturesUnordered, Stream, StreamExt},
};
use lazy_static::lazy_static;
use local_ip_address::local_ip;
use log::*;
use rand::{seq::SliceRandom, thread_rng, Rng};
use std::collections::{btree_map::Entry, BTreeMap, HashSet};
use std::convert::TryInto;
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use tokio::time::MissedTickBehavior;
use tokio::{
    net::UdpSocket,
    sync::{
        mpsc::{channel, Receiver, Sender},
        RwLock, Semaphore,
    },
    task::JoinSet,
    time::{interval, timeout, Duration},
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tokio_util::udp::UdpFramed;

#[derive(Debug, Clone)]
pub(crate) struct RegisterResult {
    reg: SocketAddr,
    dev: Option<SocketAddr>,
    dmap: Option<SocketAddr>,
    relay: Option<SocketAddr>,
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

const MTU: u32 = 1350;
lazy_static! {
    static ref P2P_RELAY_HOSTNAMES: [&'static str; 12] = [
        "p2p.reolink.com",
        "p2p1.reolink.com",
        "p2p2.reolink.com",
        "p2p3.reolink.com",
        "p2p4.reolink.com",
        "p2p5.reolink.com",
        "p2p6.reolink.com",
        "p2p7.reolink.com",
        "p2p8.reolink.com",
        "p2p9.reolink.com",
        "p2p10.reolink.com",
        "p2p11.reolink.com",
        // These following are all currently set to 127.0.0.1
        // probably reserved for future use
        // "p2p12.reolink.com",
        // "p2p13.reolink.com",
        // "p2p14.reolink.com",
        // "p2p15.reolink.com",
        // "p2p16.reolink.com",
    ];
    /// Maximum wait for a reply
    static ref MAXIMUM_WAIT: Duration = Duration::from_secs(15);
    /// Wait for tcp connections
    static ref TCP_WAIT: Duration = Duration::from_secs(4);
    /// How long to wait before resending
    static ref RESEND_WAIT: Duration = Duration::from_millis(500);

}

type Subscriber = Arc<RwLock<BTreeMap<u32, Sender<Result<(UdpDiscovery, SocketAddr)>>>>>;
type Handlers = Arc<RwLock<Vec<Sender<Result<(UdpDiscovery, SocketAddr)>>>>>;
type ArcFramedSocket = UdpFramed<BcUdpCodex, Arc<UdpSocket>>;
pub(crate) struct Discoverer {
    semaphore: Arc<Semaphore>,
    socket: Arc<UdpSocket>,
    handle: RwLock<JoinSet<Result<()>>>,
    writer: Sender<Result<(BcUdp, SocketAddr)>>,
    subsribers: Subscriber,
    handlers: Handlers,
    local_addr: SocketAddr,
    cancel: CancellationToken,
}

fn valid_ip(ip: &str) -> bool {
    !ip.is_empty() && matches!(ip.parse::<Ipv4Addr>(), Ok(addr) if addr.octets()[3] != 0)
}

fn valid_port(port: u16) -> bool {
    port != 0
}

fn valid_addr(ip_port: &IpPort) -> bool {
    valid_ip(&ip_port.ip) && valid_port(ip_port.port)
}

impl Discoverer {
    async fn new() -> Result<Discoverer> {
        let socket = Arc::new(connect().await?);
        let local_addr = socket.local_addr()?;
        let inner: ArcFramedSocket = UdpFramed::new(socket.clone(), BcUdpCodex::new());
        let cancel = CancellationToken::new();

        let (mut writer, mut reader) = inner.split();
        let mut set = JoinSet::new();
        let subsribers: Subscriber = Default::default();
        let handlers: Handlers = Default::default();

        let thread_subscriber = subsribers.clone();
        let thread_handlers = handlers.clone();
        let thread_cancel = cancel.clone();
        set.spawn(async move {
            tokio::select! {
                _ = thread_cancel.cancelled() => Result::Ok(()),
                v = async {
                    loop {
                        tokio::task::yield_now().await;
                        match reader.next().await {
                            Some(Ok((BcUdp::Discovery(bcudp), addr))) => {
                                let tid = bcudp.tid;
                                let mut needs_removal = false;
                                if let (Some(sender), true) =
                                    (thread_subscriber.read().await.get(&tid), tid > 0)
                                {
                                    if sender.send(Ok((bcudp, addr))).await.is_err() {
                                        needs_removal = true;
                                    }
                                } else {
                                    for sender in thread_handlers.write().await.iter_mut() {
                                        let _ = sender.send(Ok((bcudp.clone(), addr))).await;
                                    }
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
                                break Result::Ok(());
                            }
                            None => break Result::Ok(()),
                        }
                    }
                }=> v,
            }
        });

        let (sinker, sinker_rx) = channel::<Result<(BcUdp, SocketAddr)>>(100);
        let thread_cancel = cancel.clone();
        set.spawn(async move {
            tokio::select! {
                _ = thread_cancel.cancelled() => Result::Ok(()),
                v = async {
                    let mut sinker_rx = ReceiverStream::new(sinker_rx);
                    while let Some(Ok(packet)) = sinker_rx.next().await {
                        let _ = writer.send(packet).await;
                    }
                    Result::Ok(())
                } => v,
            }
        });

        Ok(Discoverer {
            semaphore: Arc::new(Semaphore::new(1)),
            socket,
            handle: RwLock::new(set),
            writer: sinker,
            subsribers,
            handlers,
            local_addr,
            cancel,
        })
    }

    async fn get_socket(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }

    async fn subscribe(&self, tid: u32) -> Result<Receiver<Result<(UdpDiscovery, SocketAddr)>>> {
        if tid > 0 {
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
                        // log::error!("Failed to subscribe in discovery to {:?}", tid);
                        Err(Error::SimultaneousSubscription {
                            msg_num: Some(tid as u16),
                        })
                    }
                }
            }
        } else {
            // If tid is zero we listen to all!
            let mut handlers = self.handlers.write().await;
            let (tx, rx) = channel(10);
            handlers.push(tx);
            Ok(rx)
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
        tokio::select! {
            v = async {
                loop {
                    let (reply, addr) = reply.next().await.ok_or(Error::ConnectionUnavaliable)??;
                    if let Some(result) = map(reply, addr) {
                        return Ok(result);
                    }
                }
            } => v,
            _ = tokio::time::sleep(*MAXIMUM_WAIT) => Err::<T, Error>(Error::DiscoveryTimeout),
        }
    }

    async fn send(&self, disc: BcUdp, addr: SocketAddr) -> Result<()> {
        self.writer
            .send(Ok((disc, addr)))
            .await
            .map_err(Error::from)
    }

    fn local_addr(&self) -> &SocketAddr {
        &self.local_addr
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

        let mut inter = interval(*RESEND_WAIT);
        inter.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let result = tokio::select! {
            v = async {
                // Recv while channel is viable
                while let Some(msg) = reply.next().await {
                    if let Ok((reply, addr)) = msg {
                        if let Some(result) = map(reply, addr) {
                            return Ok(result);
                        }
                    }
                }
                Err(Error::DroppedConnection)
            } => {v},
            v = async {
                // Send every inter for ever or until channel is no longer viable
                loop {
                    inter.tick().await;
                    if let Err(e) = self.send(msg.clone(), dest).await {
                        return e;
                    }
                }
            } => {Err::<T, Error>(v)},
            _ = {
                // Sleep then emit Timeout
                tokio::time::sleep(*MAXIMUM_WAIT)
            } => {
                Err::<T, Error>(Error::DiscoveryTimeout)
            }
        };

        result
    }

    async fn send_and_forget(&self, mut disc: UdpDiscovery, dest: SocketAddr) -> Result<()> {
        if disc.tid == 0 {
            // If 0 make a random one
            let target_tid = generate_tid();
            disc.tid = target_tid;
        }
        let mut inter = interval(*RESEND_WAIT);
        inter.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let msg = BcUdp::Discovery(disc);

        for _i in 0..5 {
            inter.tick().await;

            self.send(msg.clone(), dest).await?;
        }

        Ok(())
    }

    async fn client_initiated_direct(
        &self,
        uid: &str,
        client_id: i32,
        addr: SocketAddr,
    ) -> Result<ConnectResult> {
        let tid = generate_tid();

        let port = self.local_addr().port();
        let msg = UdpDiscovery {
            tid,
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

        let (camera_address, camera_id) = self
            .retry_send(msg, addr, |bc, addr| match bc {
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

        let result = ConnectResult {
            addr: camera_address,
            client_id,
            camera_id,
            sid: 0,
        };
        self.keep_alive_device(tid, &result).await;

        Ok(result)
    }

    async fn uid_lookup_all<'a>(
        &'a self,
        uid: &'a str,
    ) -> Result<impl Stream<Item = UidLookupResults> + 'a> {
        let task = tokio::task::spawn_blocking(move || {
            let mut addrs = vec![];
            for p2p_relay in P2P_RELAY_HOSTNAMES.iter() {
                addrs.append(
                    &mut format!("{}:9999", p2p_relay)
                        .to_socket_addrs()
                        .map(|i| i.collect::<Vec<SocketAddr>>())
                        .unwrap_or_else(|_| vec![]),
                );
            }
            addrs
        });
        let mut addrs = timeout(*MAXIMUM_WAIT, task).await??;
        trace!("Uid lookup to: {:?}", addrs);

        Ok(addrs
            .drain(..)
            .map(|addr| self.uid_lookup(uid, addr))
            .collect::<FuturesUnordered<_>>()
            .filter_map(|f| async { f.ok() }))
    }
    /// This function will contact the p2p relay servers
    ///
    /// It will ask each of the servers for details on a specific UID
    ///
    /// On success it will returns an async iter that yields the M2cQr that the p2p relay
    /// server has about the UID
    async fn uid_lookup(&self, uid: &str, addr: SocketAddr) -> Result<UidLookupResults> {
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
        trace!("Sending look up {:?}", msg);
        let (packet, _) = self
            .retry_send(msg, addr, |bc, addr| match bc {
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            m2c_q_r: Some(m2c_q_r),
                            ..
                        },
                } if valid_addr(&m2c_q_r.reg) => Some((m2c_q_r, addr)),
                _ => None,
            })
            .await?;
        trace!("Look up complete");
        Ok(UidLookupResults {
            reg: SocketAddr::new(packet.reg.ip.parse()?, packet.reg.port),
            relay: SocketAddr::new(packet.relay.ip.parse()?, packet.relay.port),
        })
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
        let (sid, dev, dmap, relay) = self
            .retry_send(msg, lookup.reg, |bc, socket| {
                trace!("{}", socket.ip());
                match bc {
                    UdpDiscovery {
                        tid: _,
                        payload:
                            UdpXml {
                                r2c_c_r:
                                    Some(R2cCr {
                                        dmap,
                                        dev,
                                        relay,
                                        sid,
                                        rsp,
                                        ..
                                    }),
                                ..
                            },
                    } if (dev.as_ref().map(valid_addr).unwrap_or(false)
                        || dmap.as_ref().map(valid_addr).unwrap_or(false)
                        || relay.as_ref().map(valid_addr).unwrap_or(false))
                        && rsp != -1 =>
                    {
                        Some(Ok((sid, dev, dmap, relay)))
                    }
                    // UdpDiscovery {
                    //     tid: _,
                    //     payload:
                    //         UdpXml {
                    //             r2c_c_r:
                    //                 Some(R2cCr {
                    //                     dmap,
                    //                     dev,
                    //                     relay: Some(mut relay),
                    //                     sid,
                    //                     rsp,
                    //                     ..
                    //                 }),
                    //             ..
                    //         },
                    // } if (dev
                    //     .as_ref()
                    //     .map(valid_addr)
                    //     .unwrap_or(false)
                    //     || dmap
                    //         .as_ref()
                    //         .map(valid_addr)
                    //         .unwrap_or(false)
                    //     || (relay.ip == format!("{}", socket.ip()) && relay.port == 0))
                    //     && rsp != -1 =>
                    // {
                    //     // For a relay connection if port is 0 and the ip is the current socker addr
                    //     // we use the current port
                    //     relay.port = socket.port();
                    //     Some(Ok((sid, dev, dmap, Some(relay))))
                    // }
                    UdpDiscovery {
                        tid: _,
                        payload:
                            UdpXml {
                                r2c_c_r:
                                    Some(R2cCr {
                                        dev,
                                        dmap,
                                        relay,
                                        rsp,
                                        ..
                                    }),
                                ..
                            },
                    } if (dev.as_ref().map(valid_addr).unwrap_or(false)
                        || dmap.as_ref().map(valid_addr).unwrap_or(false)
                        || relay.as_ref().map(valid_addr).unwrap_or(false))
                        && rsp == -1 =>
                    {
                        Some(Err(Error::RegisterError))
                    }
                    _ => None,
                }
            })
            .await??;

        Ok(RegisterResult {
            reg: lookup.reg,
            sid,
            client_id,
            dev: dev.and_then(|d| d.try_into().ok()),
            dmap: dmap.and_then(|d| d.try_into().ok()),
            relay: relay.and_then(|d| d.try_into().ok()),
        })
    }

    async fn device_initiated_dev(
        &self,
        register_result: &RegisterResult,
    ) -> Result<ConnectResult> {
        let (addr, local_tid, local_did) = self
            .handle_incoming(|bc, addr| {
                trace!("bc: {:?}", bc);
                match (bc, register_result) {
                    (
                        UdpDiscovery {
                            tid,
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
                        },
                        RegisterResult {
                            dmap: register_dmap,
                            sid: register_sid,
                            ..
                        },
                    ) if cid == register_result.client_id
                        && &sid == register_sid
                        && register_dmap
                            .as_ref()
                            .map(|dmap| &addr == dmap)
                            .unwrap_or(false)
                        && &conn == "local" =>
                    {
                        Some((addr, tid, did))
                    }
                    _ => None,
                }
            })
            .await?;

        let msg = UdpDiscovery {
            tid: local_tid,
            payload: UdpXml {
                c2d_a: Some(C2dA {
                    sid: register_result.sid,
                    conn: "local".to_string(),
                    cid: register_result.client_id,
                    did: local_did,
                    mtu: MTU,
                }),
                ..Default::default()
            },
        };

        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Other("Discovery already complete"))?;
        // Send and await confirm
        self.retry_send(msg, addr, |bc, _| {
            trace!("msg: {:?}", &bc);
            match bc {
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            d2c_cfm:
                                Some(D2cCfm {
                                    sid,
                                    cid,
                                    did,
                                    conn,
                                    ..
                                }),
                            ..
                        },
                } if sid == register_result.sid
                    && did == local_did
                    && cid == register_result.client_id
                    && &conn == "local" =>
                {
                    Some(())
                }
                _ => None,
            }
        })
        .await?;

        let result = ConnectResult {
            addr,
            client_id: register_result.client_id,
            sid: register_result.sid,
            camera_id: local_did,
        };

        // Confirm local to register
        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2r_cfm: Some(C2rCfm {
                    sid: result.sid,
                    cid: result.client_id,
                    did: result.camera_id,
                    rsp: 0,
                    conn: "local".to_string(),
                }),
                ..Default::default()
            },
        };

        self.send_and_forget(msg, register_result.reg).await?;

        self.keep_alive_device(local_tid, &result).await;

        self.semaphore.close();
        drop(permit);
        Ok(result)
    }

    async fn device_initiated_map(
        &self,
        register_result: &RegisterResult,
    ) -> Result<ConnectResult> {
        let (addr, local_tid, local_did) = self
            .handle_incoming(|bc, addr| {
                trace!("bc: {:?}", bc);
                match (bc, register_result) {
                    (
                        UdpDiscovery {
                            tid,
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
                        },
                        RegisterResult {
                            dmap: register_dmap,
                            sid: register_sid,
                            ..
                        },
                    ) if cid == register_result.client_id
                        && &sid == register_sid
                        && register_dmap
                            .as_ref()
                            .map(|dmap| &addr == dmap)
                            .unwrap_or(false)
                        && &conn == "map" =>
                    {
                        Some((addr, tid, did))
                    }
                    _ => None,
                }
            })
            .await?;

        let msg = UdpDiscovery {
            tid: local_tid,
            payload: UdpXml {
                c2d_a: Some(C2dA {
                    sid: register_result.sid,
                    conn: "map".to_string(),
                    cid: register_result.client_id,
                    did: local_did,
                    mtu: MTU,
                }),
                ..Default::default()
            },
        };

        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Other("Discovery already complete"))?;
        // Send and await confirm
        self.retry_send(msg, addr, |bc, _| {
            trace!("msg: {:?}", &bc);
            match bc {
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            d2c_cfm:
                                Some(D2cCfm {
                                    sid,
                                    cid,
                                    did,
                                    conn,
                                    ..
                                }),
                            ..
                        },
                } if sid == register_result.sid
                    && did == local_did
                    && cid == register_result.client_id
                    && &conn == "map" =>
                {
                    Some(())
                }
                _ => None,
            }
        })
        .await?;

        let result = ConnectResult {
            addr,
            client_id: register_result.client_id,
            sid: register_result.sid,
            camera_id: local_did,
        };

        // Confirm map to register
        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2r_cfm: Some(C2rCfm {
                    sid: result.sid,
                    cid: result.client_id,
                    did: result.camera_id,
                    rsp: 0,
                    conn: "map".to_string(),
                }),
                ..Default::default()
            },
        };

        self.send_and_forget(msg, register_result.reg).await?;

        self.keep_alive_device(local_tid, &result).await;

        self.semaphore.close();
        drop(permit);
        Ok(result)
    }

    async fn client_initiated_dev(
        &self,
        register_result: &RegisterResult,
    ) -> Result<ConnectResult> {
        let tid = generate_tid();

        let dev_addr = register_result.dev.ok_or(Error::NoDev)?;
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
            .retry_send(msg, dev_addr, |bc, addr| match bc {
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

        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Other("Discovery already complete"))?;
        // Confirm local to register
        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2r_cfm: Some(C2rCfm {
                    sid: result.sid,
                    cid: result.client_id,
                    did: result.camera_id,
                    conn: "local".to_string(),
                    rsp: 0,
                }),
                ..Default::default()
            },
        };

        self.send_and_forget(msg, register_result.reg).await?;

        self.keep_alive_device(tid, &result).await;

        self.semaphore.close();
        drop(permit);
        Ok(result)
    }

    async fn client_initiated_relay(
        &self,
        register_result: &RegisterResult,
    ) -> Result<ConnectResult> {
        let tid = generate_tid();

        let relay_addr = register_result.relay.ok_or(Error::NoDev)?;
        let msg = UdpDiscovery {
            tid,
            payload: UdpXml {
                c2d_t: Some(C2dT {
                    sid: register_result.sid,
                    cid: register_result.client_id,
                    mtu: MTU,
                    conn: "relay".to_string(),
                }),
                ..Default::default()
            },
        };

        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Other("Discovery already complete"))?;
        let (final_addr, local_did) = self
            .retry_send(msg, relay_addr, |bc, addr| match bc {
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            d2c_cfm:
                                Some(D2cCfm {
                                    cid,
                                    did,
                                    sid,
                                    conn,
                                    ..
                                }),
                            ..
                        },
                } if cid == register_result.client_id
                    && sid == register_result.sid
                    && &conn == "relay" =>
                {
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

        // Confirm relay to register
        let msg = UdpDiscovery {
            tid: 0,
            payload: UdpXml {
                c2r_cfm: Some(C2rCfm {
                    sid: result.sid,
                    cid: result.client_id,
                    did: result.camera_id,
                    conn: "relay".to_string(),
                    rsp: 0,
                }),
                ..Default::default()
            },
        };

        self.send_and_forget(msg, register_result.reg).await?;

        self.keep_alive_device(tid, &result).await;
        // self.keep_alive_relay(tid, &result).await;

        self.semaphore.close();
        drop(permit);
        Ok(result)
    }

    async fn keep_alive_device(&self, tid: u32, connect_result: &ConnectResult) {
        let client_id = connect_result.client_id;
        let camera_id = connect_result.camera_id;
        let addr = connect_result.addr;
        let mut sender = ArcFramedSocket::new(self.socket.clone(), BcUdpCodex::new());
        let mut interval = interval(Duration::from_secs(1));
        let thread_cancel = self.cancel.clone();
        self.handle.write().await.spawn(async move {
            tokio::select! {
                _ = thread_cancel.cancelled() => Result::Ok(()),
                v = async {
                    loop {
                        tokio::task::yield_now().await;
                        interval.tick().await;
                        let msg = BcUdp::Discovery(UdpDiscovery {
                            tid,
                            payload: UdpXml {
                                c2d_hb: Some(C2dHb {
                                    cid: client_id,
                                    did: camera_id,
                                }),
                                ..Default::default()
                            },
                        });
                        if sender.send((msg, addr)).await.is_err() {
                            break Result::Ok(());
                        }
                    }
                } => v,
            }
        });
    }

    #[allow(dead_code)] // Haven't seen this in the wild yet it is just speculation
    async fn keep_alive_relay(&self, tid: u32, connect_result: &ConnectResult) {
        let client_id = connect_result.client_id;
        let camera_id = connect_result.camera_id;
        let sid = connect_result.sid;
        let addr = connect_result.addr;
        let mut sender = ArcFramedSocket::new(self.socket.clone(), BcUdpCodex::new());
        let mut interval = interval(Duration::from_secs(1));
        let thread_cancel = self.cancel.clone();
        self.handle.write().await.spawn(async move {
            tokio::select! {
                _ = thread_cancel.cancelled() => Result::Ok(()),
                v = async {
                    loop {
                        tokio::task::yield_now().await;
                        interval.tick().await;
                        let msg = BcUdp::Discovery(UdpDiscovery {
                            tid,
                            payload: UdpXml {
                                c2r_hb: Some(C2rHb {
                                    sid,
                                    cid: client_id,
                                    did: camera_id,
                                }),
                                ..Default::default()
                            },
                        });
                        if sender.send((msg, addr)).await.is_err() {
                            break Result::Ok(());
                        }
                    }
                } => v,
            }
        });
    }
}

impl Drop for Discoverer {
    fn drop(&mut self) {
        log::trace!("Drop Discoverer");
        self.cancel.cancel();
        let _gt = tokio::runtime::Handle::current().enter();
        let mut handle = std::mem::take(&mut self.handle);
        tokio::task::spawn(async move { while handle.get_mut().join_next().await.is_some() {} });
        log::trace!("Dropped Discoverer");
    }
}

pub(crate) struct Discovery {
    discoverer: Discoverer,
    client_id: i32,
}

impl Discovery {
    pub(crate) async fn new() -> Result<Self> {
        Ok(Self {
            discoverer: Discoverer::new().await?,
            client_id: generate_cid(),
        })
    }

    pub(crate) async fn get_registration(&self, uid: &str) -> Result<RegisterResult> {
        let lookups = self.discoverer.uid_lookup_all(uid).await?;

        let checked_reg = Arc::new(RwLock::new(HashSet::new()));
        let reg_result = Box::pin(
            lookups
                .then(|lookup| {
                    let discoverer = &self.discoverer;
                    let client_id = self.client_id;

                    let thread_checked_reg = checked_reg.clone();
                    async move {
                        let mut locked_checked_reg = thread_checked_reg.write().await;
                        if locked_checked_reg.contains(&lookup.reg) {
                            return Result::Err(Error::Other("Already checked."));
                        }
                        locked_checked_reg.insert(lookup.reg);
                        drop(locked_checked_reg);
                        trace!("lookup: {:?}", lookup);
                        let reg_result = discoverer.register_address(uid, client_id, &lookup).await;
                        trace!("reg_result: {:?}", reg_result);
                        reg_result
                    }
                })
                .filter_map(|f| async { f.ok() }),
        )
        .next()
        .await
        .ok_or(Error::Other(
            "No reolink registers returned valid device data.",
        ))?;

        Ok(reg_result)
    }

    // Check if TCP is possible
    //
    // To do this we send a dummy login  and see if it replies with any BC packet
    pub(crate) async fn check_tcp(&self, addr: SocketAddr, channel_id: u8) -> Result<()> {
        let username = "admin";
        let password = Some("123456");
        let mut tcp_source =
            timeout(*TCP_WAIT, TcpSource::new(addr, username, password, false)).await??;

        let md5_username = md5_string(username, Md5Trunc::ZeroLast);
        let md5_password = password
            .map(|p| md5_string(p, Md5Trunc::ZeroLast))
            .unwrap_or_else(|| EMPTY_LEGACY_PASSWORD.to_owned());

        tcp_source
            .send(Bc {
                meta: BcMeta {
                    msg_id: MSG_ID_LOGIN,
                    channel_id,
                    msg_num: 0,
                    stream_type: 0,
                    response_code: 0x00,
                    class: 0x6514,
                },
                body: BcBody::LegacyMsg(LegacyMsg::LoginMsg {
                    username: md5_username,
                    password: md5_password,
                }),
            })
            .await?;

        let _bc: Bc = timeout(*TCP_WAIT, tcp_source.next())
            .await?
            .ok_or(Error::CannotInitCamera)??; // Successful recv should mean a Bc packet if not then deser will fail
        Ok(())
    }

    // Perform UDP broadcast lookup and connection
    pub(crate) async fn local(
        &self,
        uid: &str,
        mut optional_addrs: Option<Vec<SocketAddr>>,
    ) -> Result<DiscoveryResult> {
        let mut dests = get_broadcasts(&[2015, 2018])?;
        if let Some(mut optional_addrs) = optional_addrs.take() {
            trace!("Also sending to {:?}", optional_addrs);
            dests.append(&mut optional_addrs);
        }
        let discoverer_ref = &self.discoverer;
        let client_id = self.client_id;
        let mut futures = FuturesUnordered::new();
        for addr in dests.iter().copied() {
            futures.push(async move {
                discoverer_ref
                    .client_initiated_direct(uid, client_id, addr)
                    .await
            })
        }

        let connect_result;
        loop {
            match futures.next().await {
                Some(Ok(good_result)) => {
                    connect_result = good_result;
                    break;
                }
                Some(Err(_)) => {
                    continue;
                }
                None => {
                    return Err(Error::DiscoveryTimeout);
                }
            }
        }
        drop(futures);
        // drop(discoverer_ref);

        let socket = self.discoverer.get_socket().await;
        Ok(DiscoveryResult {
            socket,
            addr: connect_result.addr,
            camera_id: connect_result.camera_id,
            client_id: connect_result.client_id,
        })
    }

    // This will start remote discovery against the reolink p2p servers
    //
    // This works by registering our ip and intent to connect with the reolink
    // servers
    //
    // We will then try to connect to the camera local ip address while the camera
    // will also attempt to connec to ours
    //
    // This method is best when broadcasts are not possible but we can contact the camera
    // directly
    #[allow(unused)]
    pub(crate) async fn remote(
        &self,
        uid: &str,
        reg_result: &RegisterResult,
    ) -> Result<DiscoveryResult> {
        trace!("Start remote");
        let connect_result = tokio::select! {
            v = self.discoverer.client_initiated_dev(reg_result) => {v},
            v = self.discoverer.device_initiated_dev(reg_result) => {v},
        }?;
        trace!("connect_result: {:?}", connect_result);
        let socket = self.discoverer.get_socket().await;
        Ok(DiscoveryResult {
            socket,
            addr: connect_result.addr,
            client_id: self.client_id,
            camera_id: connect_result.camera_id,
        })
    }

    // This is similar to remote, except that it allows the camera to connect to us
    // over it's dmap (public) ip address that it has registered with reolink servers.
    //
    // This works by registering our ip address and the desire to connect with the
    // reolink servers. Data however should go to the camera's public ip address
    //
    // This method should be used when the camera is behind a NAT or firewall but we are
    // reachable
    pub(crate) async fn map(&self, reg_result: &RegisterResult) -> Result<DiscoveryResult> {
        let connect_result = self.discoverer.device_initiated_map(reg_result).await?;
        trace!("connect_result: {:?}", connect_result);

        let socket = self.discoverer.get_socket().await;
        Ok(DiscoveryResult {
            socket,
            addr: connect_result.addr,
            client_id: self.client_id,
            camera_id: connect_result.camera_id,
        })
    }

    // This will forward all connections via the reolinks servers
    //
    // This method should work if all else fails but it will require
    // us to trust reolink with our data once more...
    //
    pub(crate) async fn relay(&self, reg_result: &RegisterResult) -> Result<DiscoveryResult> {
        let connect_result = self.discoverer.client_initiated_relay(reg_result).await?;
        trace!("connect_result: {:?}", connect_result);

        let socket = self.discoverer.get_socket().await;
        Ok(DiscoveryResult {
            socket,
            addr: connect_result.addr,
            client_id: self.client_id,
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
    let mut ports: Vec<u16> = (53500..54000).collect();
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

    # Thread 4: Observed when behind a NAT on both ends of the connection

    - C->R
    ```
    <P2P>
    <C2D_T>
    <sid>526020041</sid>
    <conn>relay</conn>
    <cid>38000</cid>
    <mtu>1350</mtu>
    </C2D_T>
    </P2P>
    ```

    - R->C
    ```xml
    <P2P>
    <D2C_CFM>
    <sid>526020041</sid>
    <conn>relay</conn>
    <rsp>0</rsp>
    <cid>38000</cid>
    <did>32</did>
    <time_r>0</time_r>
    </D2C_CFM>
    </P2P>
    ```

*/

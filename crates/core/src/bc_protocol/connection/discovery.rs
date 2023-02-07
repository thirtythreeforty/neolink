//! This module handles discovery
//!
//! Given a UID find the associated IP
//!
use super::DiscoveryResult;
use crate::bcudp::codex::BcUdpCodex;
use crate::bcudp::model::*;
use crate::bcudp::xml::*;
use crate::{Error, Result};
use futures::{sink::SinkExt, stream::StreamExt};
use lazy_static::lazy_static;
use local_ip_address::local_ip;
use log::*;
use rand::{thread_rng, Rng};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use tokio::{
    net::UdpSocket,
    time::{interval, timeout, Duration},
};
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

pub(crate) type Discoverer = UdpFramed<BcUdpCodex, UdpSocket>;

impl Discovery {
    pub(crate) async fn local(uid: &str) -> Result<DiscoveryResult> {
        let mut discoverer: UdpFramed<BcUdpCodex, UdpSocket> = make_discoverer().await?;

        let client_id = generate_cid();

        let local_addr = discoverer.get_ref().local_addr()?;
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

        let dests = get_broadcasts(&[2015, 2018])?;
        let (camera_address, camera_id) =
            retry_send(&mut discoverer, msg, &dests, |bc, addr| match bc {
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
            socket: discoverer.into_inner(),
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
    async fn remote(uid: &str) -> Result<DiscoveryResult> {
        let mut discoverer: UdpFramed<BcUdpCodex, UdpSocket> = make_discoverer().await?;

        let client_id = generate_cid();

        let local_addr = discoverer.get_ref().local_addr()?;
        let local_port = local_addr.port();

        let reg_packet = get_register(&mut discoverer, uid).await?;

        let register_address = reg_packet.reg;
        let relay_address = reg_packet.relay;
        let log_address = reg_packet.log;

        let default_local_address = local_ip().expect("There to be a local ip");
        debug!("Register address found: {:?}", register_address);
        debug!("Registering this address: {:?}", default_local_address);

        let local_family = if local_addr.ip().is_ipv4() { 4 } else { 6 };
        let msg = UdpDiscovery {
            tid: generate_tid(),
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
        let (device_sid, dev_loc) = retry_send(
            &mut discoverer,
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

        let msg = UdpDiscovery {
            tid: generate_tid(),
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

        let device_id = retry_send(
            &mut discoverer,
            msg,
            &[SocketAddr::new(dev_loc.ip.parse()?, dev_loc.port)],
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
        )
        .await?;

        debug!("Got device ID as: {:?}", device_id);

        let msg = UdpDiscovery {
            tid: generate_tid(),
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

        retry_send(
            &mut discoverer,
            msg,
            &[SocketAddr::new(log_address.ip.parse()?, log_address.port)],
            |_, _| Some(()),
        )
        .await?;

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

        retry_send(
            &mut discoverer,
            msg,
            &[SocketAddr::new(dev_loc.ip.parse()?, dev_loc.port)],
            |_, _| Some(()),
        )
        .await?;

        Ok(DiscoveryResult {
            socket: discoverer.into_inner(),
            addr: SocketAddr::new(dev_loc.ip.parse()?, dev_loc.port),
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
    async fn relay(uid: &str) -> Result<DiscoveryResult> {
        let mut discoverer: UdpFramed<BcUdpCodex, UdpSocket> = make_discoverer().await?;

        let client_id = generate_cid();

        let local_addr = discoverer.get_ref().local_addr()?;
        let local_port = local_addr.port();

        let reg_packet = get_register(&mut discoverer, uid).await?;

        let register_address = reg_packet.reg;
        let relay_address = reg_packet.relay;
        let log_address = reg_packet.log;

        let register_socket = SocketAddr::new(register_address.ip.parse()?, register_address.port);

        let default_local_address = local_ip().expect("There to be a local ip");
        debug!("Register address found: {:?}", register_address);
        debug!("Registering this address: {:?}", default_local_address);

        let local_family = if local_addr.ip().is_ipv4() { 4 } else { 6 };
        let msg = UdpDiscovery {
            tid: generate_tid(),
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

        let (device_sid, dmap) =
            retry_send(&mut discoverer, msg, &[register_socket], |bc, _| match bc {
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
                } if !dmap.ip.is_empty() && dmap.port > 0 && cid == client_id => Some((sid, dmap)),
                UdpDiscovery {
                    tid: _,
                    payload:
                        UdpXml {
                            r2c_c_r: Some(R2cCr { dmap, sid, .. }),
                            ..
                        },
                } if !dmap.ip.is_empty() && dmap.port > 0 => Some((sid, dmap)),
                _ => None,
            })
            .await?;

        debug!("Register revealed dmap address as SID: {:?}", device_sid);
        debug!("Register revealed dmap address as IP: {:?}", dmap);

        let msg = UdpDiscovery {
            tid: generate_tid(),
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

        let device_id = retry_send(&mut discoverer, msg, &[register_socket], |bc, _| match bc {
            UdpDiscovery {
                tid: _,
                payload:
                    UdpXml {
                        d2c_cfm: Some(D2cCfm { cid, did, .. }),
                        ..
                    },
            } if cid == client_id => Some(did),
            _ => None,
        })
        .await?;

        debug!("Got device ID as: {:?}", device_id);

        let msg = UdpDiscovery {
            tid: generate_tid(),
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

        retry_send(
            &mut discoverer,
            msg,
            &[SocketAddr::new(log_address.ip.parse()?, log_address.port)],
            |_, _| Some(()),
        )
        .await?;

        Ok(DiscoveryResult {
            socket: discoverer.into_inner(),
            addr: register_socket,
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
async fn get_register(discoverer: &mut Discoverer, uid: &str) -> Result<M2cQr> {
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
        tid: generate_tid(),
        payload: UdpXml {
            c2m_q: Some(C2mQ {
                uid: uid.to_string(),
                os: "MAC".to_string(),
            }),
            ..Default::default()
        },
    };
    let (packet, addr) = retry_send(discoverer, msg, addrs.as_slice(), |bc, addr| match bc {
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

async fn make_discoverer() -> Result<Discoverer> {
    unimplemented!()
}

async fn retry_send<F, T>(
    discoverer: &mut Discoverer,
    disc: UdpDiscovery,
    dests: &[SocketAddr],
    map: F,
) -> Result<T>
where
    F: Fn(UdpDiscovery, SocketAddr) -> Option<T>,
{
    let target_tid = disc.tid;
    let msg = BcUdp::Discovery(disc);
    let mut inter = interval(Duration::from_millis(50));
    for _i in 0..5 {
        inter.tick().await;
        for payload in dests.iter().map(|addr| (&msg, *addr)) {
            let _ = discoverer.send(payload).await;
        }

        if let Ok(Some(Ok((reply, addr)))) =
            timeout(Duration::from_millis(50), discoverer.next()).await
        {
            match reply {
                BcUdp::Discovery(disc @ UdpDiscovery { tid, .. }) if tid == target_tid => {
                    if let Some(result) = map(disc, addr) {
                        return Ok(result);
                    }
                }
                smt => debug!("Udp Discovery got this unexpected BcUdp {:?}", smt),
            }
        }
    }
    Err(Error::DiscoveryTimeout)
}

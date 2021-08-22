/// This module handles the data from Bc->BcUdp->UdpSocket
///
/// - It recieves Bc as Vec<u8> (after serialize)
/// - It wraps it in BcUdp packet
/// - It retransmits when camera dosen't acknoledge
/// - It sends UdpAck to acknoledge recieved packets
///
use super::{aborthandle::AbortHandle, discover::UdpDiscover, SOCKET_WAIT_TIME, WAIT_TIME};
use crate::bcudp::{model::*, xml::*};
use crossbeam_channel::{Receiver, SendError, Sender, TryRecvError};
use err_derive::Error;
use log::*;
use std::collections::HashMap;
use std::io::{Error as IoError, ErrorKind};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicU32, Ordering};
use time::OffsetDateTime;

#[derive(Debug, Error)]
pub enum TransmitError {
    /// Error raised when IO fails such as when the connection is lost
    #[error(display = "Io error")]
    Io(#[error(source)] IoError),

    /// Error raised when socket recv IO fails such as when the connection is lost
    #[error(display = "Socket Recv error")]
    SocketRecv { err: std::io::Error },

    /// Error raised when socker IO fails such as when the connection is lost
    #[error(display = "Socket Send error")]
    SocketSend { err: std::io::Error },

    /// Error raised when camera sends a disconnect request
    #[error(display = "Camera Requested Disconnected")]
    Disc,

    /// Error raised during deserlization
    #[error(display = "BcUdp Deserialization error")]
    BcUdpDeserialization(#[error(source)] crate::bcudp::de::Error),

    /// Error raised during serlization
    #[error(display = "BcUdp Serialization error")]
    BcUdpSerialization(#[error(source)] crate::bcudp::ser::Error),

    /// Error raised during send
    #[error(display = "BcUdp Send to Channel Failed")]
    BcUdpSend(#[error(source)] SendError<Vec<u8>>),

    /// Error raised during recv
    #[error(display = "BcUdp Recv from Channel Failed")]
    BcUdpRecv(#[error(source)] TryRecvError),
}

type Result<T> = std::result::Result<T, TransmitError>;

pub struct UdpTransmit {
    client_wants: AtomicU32,
    camera_acknowledged: AtomicU32,
    client_sent: AtomicU32,
}

impl UdpTransmit {
    pub fn new() -> Self {
        Self {
            client_wants: Default::default(),
            camera_acknowledged: Default::default(),
            client_sent: Default::default(),
        }
    }

    pub fn poll_read(
        &self,
        socket: &UdpSocket,
        discovery_result: &UdpDiscover,
        send_to_incoming: &Sender<Vec<u8>>,
    ) -> Result<()> {
        // Handle recv from `socket` and send to `send_to_incoming`
        let mut buf = vec![0; discovery_result.mtu as usize];
        match socket.recv(&mut buf) {
            Ok(amount_recv) => {
                let bcudp_msg = BcUdp::deserialize(&buf[0..amount_recv])?;
                match bcudp_msg {
                    BcUdp::Discovery(UdpDiscovery {
                        tid,
                        payload:
                            UdpXml {
                                d2c_disc: Some(D2cDisc { cid, did }),
                                ..
                            },
                        ..
                    }) if cid == discovery_result.client_id
                        && did == discovery_result.camera_id =>
                    {
                        // Reply with out C2D_Disc and end
                        let bcudp_msg = BcUdp::Discovery(UdpDiscovery {
                            tid,
                            payload: UdpXml {
                                c2d_disc: Some(C2dDisc {
                                    cid: discovery_result.client_id,
                                    did: discovery_result.camera_id,
                                }),
                                ..Default::default()
                            },
                        });

                        let mut buf = vec![];
                        bcudp_msg.serialize(&mut buf)?;
                        socket
                            .send(&buf)
                            .map_err(|e| TransmitError::SocketSend { err: e })?;
                        return Err(TransmitError::Disc);
                    }
                    BcUdp::Discovery(packet) => {
                        debug!("Got unexpected discovery packet: {:?}", packet)
                    }
                    BcUdp::Ack(UdpAck {
                        connection_id,
                        packet_id,
                        ..
                    }) if connection_id == discovery_result.client_id => {
                        self.camera_acknowledged
                            .fetch_max(packet_id, Ordering::Acquire);
                    }
                    BcUdp::Ack(UdpAck { .. }) => {
                        debug!("Got a UdpAck for another client id");
                    }
                    BcUdp::Data(UdpData {
                        connection_id: cid,
                        packet_id,
                        payload,
                    }) if cid == discovery_result.client_id => {
                        // Seperated as let/if due to clippy recommend
                        trace!("Recieving UdpData packet {}", packet_id);
                        let updated = self
                            .client_wants
                            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                                if v == packet_id {
                                    Some(v + 1)
                                } else {
                                    None
                                }
                            })
                            .is_ok();
                        if updated {
                            send_to_incoming.send(payload)?;
                        }
                    }
                    BcUdp::Data(UdpData { .. }) => {
                        debug!("Got UdpData for another client id");
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                // Just a timeout
            }
            Err(e) => {
                // Should stop
                return Err(TransmitError::SocketRecv { err: e });
            }
        }
        Ok(())
    }

    pub fn poll_write(
        &self,
        socket: &UdpSocket,
        discovery_result: &UdpDiscover,
        get_from_outgoing: &Receiver<Vec<u8>>,
        outgoing_history: &mut HashMap<u32, Vec<u8>>,
    ) -> Result<()> {
        // Handle Retransmit
        let camera_has = self.camera_acknowledged.load(Ordering::Relaxed);
        let client_sent = self.client_sent.load(Ordering::Relaxed);
        if camera_has < client_sent {
            debug!("Resending {:?}", (camera_has + 1)..(client_sent + 1));
            for packet_id in (camera_has + 1)..(client_sent + 1) {
                if let Some(payload) = &outgoing_history.get(&packet_id) {
                    debug!("    - Sending {}", packet_id);
                    socket
                        .send(payload)
                        .map_err(|e| TransmitError::SocketSend { err: e })?;
                }
                std::thread::sleep(SOCKET_WAIT_TIME);
            }
        }

        // Handle recv from `send_to_incoming` and send to `socket`
        match get_from_outgoing.try_recv() {
            Ok(payload) => {
                let packet_id = self.client_sent.fetch_add(1, Ordering::Relaxed);
                let bcudp_msg = BcUdp::Data(UdpData {
                    connection_id: discovery_result.camera_id,
                    packet_id,
                    payload,
                });

                let mut buf = vec![];
                bcudp_msg.serialize(&mut buf)?;
                socket
                    .send(&buf)
                    .map_err(|e| TransmitError::SocketSend { err: e })?;
                outgoing_history.insert(packet_id, buf);
            }
            Err(TryRecvError::Empty) => {}
            Err(e) => return Err(e.into()),
        }

        // Handle acknoledgment
        let client_wants = self.client_wants.load(Ordering::Relaxed);
        if client_wants > 0 {
            let bcack_msg = BcUdp::Ack(UdpAck {
                connection_id: discovery_result.camera_id,
                packet_id: client_wants,
            });

            let mut buf = vec![];
            bcack_msg.serialize(&mut buf)?;
            socket
                .send(&buf)
                .map_err(|e| TransmitError::SocketSend { err: e })?;
        }
        Ok(())
    }
}

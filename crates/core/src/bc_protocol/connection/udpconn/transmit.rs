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
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Mutex,
};
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
    client_recieved: ClientRecieved,
    client_sent: ClientSent,
}

#[derive(Default)]
struct ClientSent {
    buffer: Mutex<HashMap<u32, Vec<u8>>>,
    last_sent: Mutex<Option<u32>>,
}

impl ClientSent {
    fn get_next_packet_id(&self) -> u32 {
        let mut lock = self.last_sent.lock().unwrap();
        if let Some(current) = lock.as_ref() {
            *lock = Some(current + 1);
        } else {
            *lock = Some(0);
        }
        lock.unwrap()
    }

    fn register_payload(&self, packet_id: u32, payload: Vec<u8>) {
        self.buffer
            .lock()
            .unwrap()
            .entry(packet_id)
            .or_insert(payload);
    }

    // fn acknoledge_to(&self, packet_id: u32) {
    //     self.buffer.lock().unwrap().retain(|&k, _| k > packet_id);
    // }
    //
    // fn acknoledge_these(&self, packet_ids: Vec<u32>) {
    //     let locked = self.buffer.lock().unwrap();
    //     for packet_id in packet_ids {
    //         let _ = locked.remove(&packet_id);
    //     }
    // }

    fn acknoledge_from_ack_data(&self, start: u32, payload: Vec<u8>) {
        let mut locked = self.buffer.lock().unwrap();
        locked.retain(|&k, _| k > start);

        for (idx, &value) in payload.iter().enumerate() {
            let packet_id = (start + 1) + idx as u32;
            if value > 0 {
                locked.remove(&packet_id);
            }
        }
    }

    fn lock(&self) -> ClientSentLocked {
        ClientSentLocked::new(self)
    }
}

struct ClientSentLocked<'a> {
    locked: std::sync::MutexGuard<'a, HashMap<u32, Vec<u8>>>,
}

impl<'a> ClientSentLocked<'a> {
    fn new(source: &'a ClientSent) -> Self {
        let locked = source.buffer.lock().unwrap();
        Self { locked }
    }

    fn needs_resend(&self) -> ResendIter {
        ResendIter {
            iter: self.locked.iter(),
        }
    }
}

struct ResendIter<'a> {
    iter: std::collections::hash_map::Iter<'a, u32, Vec<u8>>,
}

impl<'a> std::iter::Iterator for ResendIter<'a> {
    type Item = (u32, &'a Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        let (k, v) = self.iter.next()?;
        Some((*k, v))
    }
}

#[derive(Default)]
struct ClientRecieved {
    buffer: Mutex<HashMap<u32, Vec<u8>>>,
    consumed: Mutex<Option<u32>>,
}

impl ClientRecieved {
    // Consume all contigious packets
    fn consume(&self) -> Vec<Vec<u8>> {
        let mut locked = self.buffer.lock().unwrap();
        let mut results = vec![];
        if locked.keys().max().is_some() {
            // Just check there is some data
            let mut packet_id_loc = self.consumed.lock().unwrap();
            let mut next_packet_id = packet_id_loc.map(|v| v + 1).unwrap_or(0);
            while let Some(payload) = locked.remove(&next_packet_id) {
                *packet_id_loc = Some(next_packet_id);
                results.push(payload);
                next_packet_id = packet_id_loc.unwrap() + 1;
            }
        }
        results
    }

    // Revieve a new packet only update if entry is new
    fn receive(&self, packet_id: u32, payload: Vec<u8>) {
        let mut locked = self.buffer.lock().unwrap();
        let _ = locked.entry(packet_id).or_insert(payload);
    }

    fn get_needed_packets(&self) -> Option<(u32, Vec<u8>)> {
        let locked = self.buffer.lock().unwrap();
        let packet_id_loc = self.consumed.lock().unwrap();

        // If this is none we have recieved no packets so we cannot
        // ack anything anyways. Just return None
        let mut start = packet_id_loc.as_ref().copied()?;

        // Find the last contigous packet
        while locked.contains_key(&(start + 1)) {
            start += 1;
        }

        // Find last packet in buffer
        let vec = if let Some(end) = locked.keys().max() {
            let mut vec = vec![];
            for i in (start + 1)..(end + 1) {
                if locked.contains_key(&i) {
                    vec.push(1)
                } else {
                    vec.push(0)
                }
            }
            vec
        } else {
            vec![]
        };

        Some((start, vec))
    }
}

impl UdpTransmit {
    pub fn new() -> Self {
        Self {
            client_recieved: Default::default(),
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
                        // Reply with C2D_Disc and end
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
                        payload,
                    }) if connection_id == discovery_result.client_id => {
                        self.client_sent
                            .acknoledge_from_ack_data(packet_id, payload);
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
                        self.client_recieved.receive(packet_id, payload);

                        for next_payload in self.client_recieved.consume().drain(..) {
                            send_to_incoming.send(next_payload)?;
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
    ) -> Result<()> {
        // Handle Retransmit
        for (_, payload) in self.client_sent.lock().needs_resend() {
            socket
                .send(payload)
                .map_err(|e| TransmitError::SocketSend { err: e })?;
            std::thread::sleep(SOCKET_WAIT_TIME);
        }

        // Handle recv from `send_to_incoming` and send to `socket`
        match get_from_outgoing.try_recv() {
            Ok(payload) => {
                let packet_id = self.client_sent.get_next_packet_id();
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
                self.client_sent.register_payload(packet_id, buf);
            }
            Err(TryRecvError::Empty) => {}
            Err(e) => return Err(e.into()),
        }

        // Handle acknoledgment
        let client_wants = self.client_recieved.get_needed_packets();
        if let Some((client_wants, also_wants)) = client_wants {
            let bcack_msg = BcUdp::Ack(UdpAck {
                connection_id: discovery_result.camera_id,
                packet_id: client_wants,
                payload: also_wants,
            });

            let mut buf = vec![];
            bcack_msg.serialize(&mut buf)?;
            socket
                .send(&buf)
                .map_err(|e| TransmitError::SocketSend { err: e })?;
        }

        std::thread::sleep(SOCKET_WAIT_TIME);
        Ok(())
    }
}

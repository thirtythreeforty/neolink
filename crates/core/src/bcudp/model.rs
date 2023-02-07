//! The BcUdp Model
//!

use super::xml::*;

/// Top level udp packet
#[derive(Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum BcUdp {
    /// Packet from the negotiate stage when connection info is exchanged
    Discovery(UdpDiscovery),
    /// Packet to acknowledge receipt of a data packet
    Ack(UdpAck),
    /// Packet containing the data (or part of the data) of a Bc packet
    Data(UdpData),
}

impl BcUdp {
    /// Gets a connection ID for any kind packet
    pub fn get_connection_id(&self) -> i32 {
        match self {
            Self::Discovery(_) => 0,
            Self::Ack(data) => data.connection_id,
            Self::Data(data) => data.connection_id,
        }
    }
}

/// Magic for the UDP Discovery packet
pub const MAGIC_HEADER_UDP_NEGO: u32 = 0x2a87cf3a;

/// The Discovery packet is sent and received to init a connection
#[derive(Debug, PartialEq, Eq)]
pub struct UdpDiscovery {
    // The packet also contains these header fields not deserialized into this struct:
    // 4 Bytes Magic
    // 4 Byte payload size
    // 4 Bytes unknown always `01000000`
    /// The transmission id is unique to a message and used as an encryption key
    pub tid: u32,
    // The checksum of the payload
    // pub checksum: u32,
    /// The payload
    pub payload: UdpXml,
}

/// Magic for the UDP Ack packet
pub const MAGIC_HEADER_UDP_ACK: u32 = 0x2a87cf20;

/// Send to acknoledge a [`UdpData`] packet. If this is not sent then the camera will
/// resend the packet
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UdpAck {
    /// The connection ID
    ///
    /// This is negotiated during [`UdpDiscovery`] as cid for the client and did for the camera
    ///
    /// When receiving from the camera it will be cid
    ///
    /// When sending to the camera it should be did
    ///
    /// We use i32 because when we send the connection id to the reolink
    /// register and then download the same connection id from the register_address
    /// it comes back in an xml that is encoded as i32 (i.e. can be negative string)
    pub connection_id: i32,
    // Unknown 4 bytes always 0
    /// The ID of the last data packet [`UdpData`]
    pub packet_id: u32,
    // 2 Bytes Unknown: Observed values `00000000`, `d6010000`, `d7160000` `09e00000`
    //                  Unknown but seems to change randomly every second
    // 2 Bytes size of a payload
    //
    /// Payload of `00 01 01 01 01` where `01` is added after every repeat
    ///
    /// This is a truth table of packets after `packet_id` that have not been recieved
    pub payload: Vec<u8>,
}

/// Magic for the UDP Data packet
pub const MAGIC_HEADER_UDP_DATA: u32 = 0x2a87cf10;

/// Contains the data of a [`crate::bc::model::Bc`] packet
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UdpData {
    /// The connection ID of the other party
    ///
    /// This is negotiated during [`UdpDiscovery`] as cid for the client and did for the camera
    ///
    /// When receiving from the camera it will be cid
    ///
    /// When sending to the camera it should be did
    ///
    /// We use i32 because when we send the connection id to the reolink
    /// register and then download the same connection id from the register_address
    /// it comes back in an xml that is encoded as i32 (i.e. can be negative string)
    pub connection_id: i32,
    // Unknown 4 bytes always 0
    /// The ID of the data packet
    pub packet_id: u32,
    // Unknown 4 bytes always 0
    // 4 Byte payload size
    /// The payload
    pub payload: Vec<u8>,
}

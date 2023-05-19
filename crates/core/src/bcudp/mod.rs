//! This module contains the protocol for dealing with UDP.
//!
//! There are three types of packets
//!
//! - Discovery
//! - Ack
//! - Data
//!
//! ---
//!
//! **Discovery**: Deals with setting up the initial connection and including their
//! connection IDs and the MTU
//!
//! ---
//!
//! **Ack**: Is sent after every packet is recieved
//!
//! ---
//!
//! **Data**: Contains a Bc packet payload. This is a stream and one Bc Packet may
//! be split accross multiple UDP Data packets
//!

pub(crate) mod codex;
mod crc;
/// Functions to deserialize udp packets
pub mod de;
/// Contains the model describing the top level structures
pub mod model;
/// Functions to serialize udp packets
pub mod ser;
/// Contains the udp related xml payloads
pub mod xml;
/// Constains routines to de/encrypt udp xml
mod xml_crypto;

//! The Baichuan message format is a 20 byte header, the contents of which vary between legacy and
//! modern messages:
//!
//!
//!
//! This header is followed by the message body.  In legacy messages, the bodies are
//! message-specific binary formats.  Currently, we only attempt to interpret the legacy login
//! message, as it is all that is needed to upgrade to the modern XML-based messages.  Modern
//! messages are either "encrypted" XML (the encryption is a simple XOR routine or AES)
//! or binary data.
//!
//! ---
//!
//! # Payloads
//! Messages contain one-two payloads seperated by the payload_offset in the header
//!
//! ## Extension Payload
//! The first payload prior to the payload_offset is the extension xml
//!
//! This contains meta data on the following payload such as channel_id or content type
//! (xml or binary)
//!
//! ## Payload
//! The second payload which is the primary payload coming after the payload offset
//! depends on the MsgID.
//!
//! It is usually XML except in the case of video and talk MsgIDs
//! which are binary data in the bc media packet format
//!

/// Contains the structure of the messages such as headers and payloads
pub mod model;

/// Contains code related to the deserialisation of the bc packets
pub mod de;
/// `Contains code related to the serialisation of the bc packets
pub mod ser;
/// Contains the structs for the know xmls of payloads and extension
pub mod xml;

mod xml_crypto;

pub(crate) mod codex;

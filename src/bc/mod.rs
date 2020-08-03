//! The Baichuan message format is a 20 byte header, the contents of which vary between legacy and
//! modern messages:
//!
//!
//!
//! This header is followed by the message body.  In legacy messages, the bodies are
//! message-specific binary formats.  Currently, we only attempt to interpret the legacy login
//! message, as it is all that is needed to upgrade to the modern XML-based messages.  Modern
//! messages are either "encrypted" XML (the encryption is a simple XOR routine) or binary data.
//! All message IDs start out as XML, but can be statefully switched to binary with a special XML
//! "Extension" message.

pub mod media_packet;
pub mod model;

pub mod de;
pub mod ser;
pub mod xml;

mod xml_crypto;

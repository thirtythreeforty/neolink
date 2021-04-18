pub use super::xml::{BcPayloads, BcXml, Extension};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub(super) const MAGIC_HEADER: u32 = 0xabcdef0;

pub const MSG_ID_LOGIN: u32 = 1;
pub const MSG_ID_VIDEO: u32 = 3;
pub const MSG_ID_VERSION: u32 = 80;
pub const MSG_ID_PING: u32 = 93;
pub const MSG_ID_GET_GENERAL: u32 = 104;
pub const MSG_ID_SET_GENERAL: u32 = 105;

pub const EMPTY_LEGACY_PASSWORD: &str =
    "\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

#[derive(Debug, PartialEq, Eq)]
pub struct Bc {
    pub meta: BcMeta,
    pub body: BcBody,
}

#[derive(Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum BcBody {
    LegacyMsg(LegacyMsg),
    ModernMsg(ModernMsg),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ModernMsg {
    pub extension: Option<Extension>,
    pub payload: Option<BcPayloads>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LegacyMsg {
    LoginMsg { username: String, password: String },
    UnknownMsg,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct BcHeader {
    pub body_len: u32,
    pub msg_id: u32,
    pub channel_id: u8,
    pub stream_type: u8,
    pub msg_num: u16,
    pub response_code: u16,
    pub class: u16,
    pub payload_offset: Option<u32>,
}

/// The components of the Baichuan TLV header that are not
/// descriptions of the Body (the application dictates these)
#[derive(Debug, PartialEq, Eq)]
pub struct BcMeta {
    pub msg_id: u32,
    pub channel_id: u8,
    pub stream_type: u8,
    pub response_code: u16,
    pub msg_num: u16,
    pub class: u16,
}

/// The components of the Baichuan header that must be filled out after the body is serialized, or
/// is needed for the deserialization of the body (strictly part of the wire format of the message)
#[derive(Debug, PartialEq, Eq)]
pub(super) struct BcSendInfo {
    pub body_len: u32,
    pub payload_offset: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum EncryptionProtocol {
    Unencrypted,
    BCEncrypt,
    Aes(Option<[u8; 16]>),
}

#[derive(Debug)]
pub struct BcContext {
    pub(super) in_bin_mode: HashSet<u16>,
    // Arc<Mutex<EncryptionProtocol>> because it is shared between context
    // and connection for deserialisation and serialistion respectivly
    pub(super) encryption_protocol: Arc<Mutex<EncryptionProtocol>>,
}

impl Bc {
    /// Convenience function that constructs a modern Bc message from the given meta and XML, with
    /// no binary payload.
    pub fn new_from_xml(meta: BcMeta, xml: BcXml) -> Bc {
        Bc {
            meta,
            body: BcBody::ModernMsg(ModernMsg {
                extension: None,
                payload: Some(BcPayloads::BcXml(xml)),
            }),
        }
    }

    pub fn new_from_ext(meta: BcMeta, xml: Extension) -> Bc {
        Bc {
            meta,
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(xml),
                payload: None,
            }),
        }
    }

    pub fn new_from_meta(meta: BcMeta) -> Bc {
        Bc {
            meta,
            body: BcBody::ModernMsg(ModernMsg {
                extension: None,
                payload: None,
            }),
        }
    }

    pub fn new_from_ext_xml(meta: BcMeta, ext: Extension, xml: BcXml) -> Bc {
        Bc {
            meta,
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(ext),
                payload: Some(BcPayloads::BcXml(xml)),
            }),
        }
    }
}

impl BcContext {
    pub fn new(encryption_protocol: Arc<Mutex<EncryptionProtocol>>) -> BcContext {
        BcContext {
            in_bin_mode: HashSet::new(),
            encryption_protocol,
        }
    }

    pub fn set_encrypted(&mut self, encryption_protocol: EncryptionProtocol) {
        *(self.encryption_protocol.lock().unwrap()) = encryption_protocol;
    }

    pub fn get_encrypted(&self) -> EncryptionProtocol {
        (*(self.encryption_protocol.lock().unwrap())).clone()
    }
}

impl BcHeader {
    pub fn is_modern(&self) -> bool {
        // Most modern messages have an extra word at the end of the header; this
        // serves as the start offset of the appended payload data, if any.
        // A notable exception is the encrypted reply to the login message;
        // in this case the message is modern (with XML encryption etc), but there is
        // no extra word.
        // Here are the message classes:
        // 0x6514: legacy, no  bin offset (initial login message, encrypted or not)
        // 0x6614: modern, no  bin offset (reply to encrypted 0x6514 login)
        // 0x6414: modern, has bin offset, encrypted if supported (re-sent login message)
        // 0x0000, modern, has bin offset (most modern messages)
        self.class != 0x6514
    }

    pub fn to_meta(&self) -> BcMeta {
        BcMeta {
            msg_id: self.msg_id,
            msg_num: self.msg_num,
            channel_id: self.channel_id,
            response_code: self.response_code,
            stream_type: self.stream_type,
            class: self.class,
        }
    }

    pub fn from_meta(meta: &BcMeta, body_len: u32, payload_offset: Option<u32>) -> BcHeader {
        BcHeader {
            payload_offset,
            body_len,
            msg_id: meta.msg_id,
            channel_id: meta.channel_id,
            stream_type: meta.stream_type,
            response_code: meta.response_code,
            msg_num: meta.msg_num,
            class: meta.class,
        }
    }
}

pub(super) fn has_payload_offset(class: u16) -> bool {
    // See BcHeader::is_modern() for a description of which packets have the bin offset
    class == 0x6414 || class == 0x0000
}

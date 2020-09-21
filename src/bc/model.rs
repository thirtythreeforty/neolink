use super::xml::BcXml;
use std::collections::HashSet;

pub(super) const MAGIC_HEADER: u32 = 0xabcdef0;

pub const MSG_ID_LOGIN: u32 = 1;
pub const MSG_ID_VIDEO: u32 = 3;
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
pub enum BcBody {
    LegacyMsg(LegacyMsg),
    ModernMsg(ModernMsg),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ModernMsg {
    pub xml: Option<BcXml>,
    pub binary: Option<Vec<u8>>,
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
    pub enc_offset: u32,
    pub encrypted: bool,
    pub class: u16,
    pub bin_offset: Option<u32>,
}

/// The components of the Baichuan TLV header that are not
/// descriptions of the Body (the application dictates these)
#[derive(Debug, PartialEq, Eq)]
pub struct BcMeta {
    pub msg_id: u32,
    pub client_idx: u32,
    pub class: u16,
    pub encrypted: bool,
}

/// The components of the Baichuan header that must be filled out after the body is serialized, or
/// is needed for the deserialization of the body (strictly part of the wire format of the message)
#[derive(Debug, PartialEq, Eq)]
pub(super) struct BcSendInfo {
    pub body_len: u32,
    pub bin_offset: Option<u32>,
}

#[derive(Debug)]
pub struct BcContext {
    pub(super) in_bin_mode: HashSet<u32>,
}

impl Bc {
    /// Convenience function that constructs a modern Bc message from the given meta and XML, with
    /// no binary payload.
    pub fn new_from_xml(meta: BcMeta, xml: BcXml) -> Bc {
        Bc {
            meta,
            body: BcBody::ModernMsg(ModernMsg {
                xml: Some(xml),
                binary: None,
            }),
        }
    }
}

impl Default for BcContext {
    fn default() -> BcContext {
        Self::new()
    }
}

impl BcContext {
    pub fn new() -> BcContext {
        BcContext {
            in_bin_mode: HashSet::new(),
        }
    }
}

impl BcHeader {
    pub fn is_modern(&self) -> bool {
        // Most modern messages have an extra word at the end of the header; this
        // serves as the start offset of the appended binary data, if any.
        // A notable exception is the encrypted reply to the login message;
        // in this case the message is modern (with XML encryption etc), but there is
        // no extra word.
        // Here are the message classes:
        // 0x6514: legacy, no  bin offset (initial login message, encrypted or not)
        // 0x6614: modern, no  bin offset (reply to encrypted 0x6514 login)
        // 0x6414: modern, has bin offset, always encrypted (re-sent login message)
        // 0x0000, modern, has bin offset (most modern messages)
        self.class != 0x6514
    }

    pub fn is_encrypted(&self) -> bool {
        self.encrypted || self.class == 0x6414
    }

    pub fn to_meta(&self) -> BcMeta {
        BcMeta {
            msg_id: self.msg_id,
            client_idx: self.enc_offset,
            class: self.class,
            encrypted: self.encrypted,
        }
    }

    pub fn from_meta(meta: &BcMeta, body_len: u32, bin_offset: Option<u32>) -> BcHeader {
        BcHeader {
            bin_offset,
            body_len,
            msg_id: meta.msg_id,
            enc_offset: meta.client_idx,
            class: meta.class,
            encrypted: meta.encrypted,
        }
    }
}

pub(super) fn has_bin_offset(class: u16) -> bool {
    // See BcHeader::is_modern() for a description of which packets have the bin offset
    class == 0x6414 || class == 0x0000
}

use super::xml::BcXml;
use log::trace;
use std::cmp::min;
use std::collections::HashSet;
use std::convert::TryInto;

pub(super) const MAGIC_HEADER: u32 = 0xabcdef0;

pub const MSG_ID_LOGIN: u32 = 1;
pub const MSG_ID_VIDEO: u32 = 3;
pub const MSG_ID_PING: u32 = 93;

pub const CHUNK_SIZE: usize = 40000;

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

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub enum BinaryDataKind {
    VideoDataIframe,
    VideoDataPframe,
    AudioDataAac,
    AudioDataAdpcm,
    InfoData,
    Invalid,
    Continue,
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BinaryData {
    pub data: Vec<u8>,
}

impl BinaryData {
    pub fn body(&self) -> &[u8] {
        let lower_limit = self.header_size();
        let upper_limit = self.data_size() + lower_limit;
        let len = self.len();
        if !self.complete() {
            unreachable!(); // An incomplete packet should be discarded not read, panic if we try
        }
        trace!(
            "Sending data from {} to {} of {}",
            lower_limit,
            upper_limit,
            len
        );
        &self.data[lower_limit..upper_limit]
    }

    pub fn complete(&self) -> bool {
        if self.len() < self.header_size() {
            return false;
        }
        let full_data_len = self.len() - self.header_size();
        full_data_len >= self.data_size() && full_data_len < self.data_size() + 8
    }

    pub fn expected_num_packet(&self) -> usize {
        self.data_size() / CHUNK_SIZE + 1
    }

    pub fn header(&self) -> &[u8] {
        let lower_limit = 0;
        let upper_limit = self.header_size() + lower_limit;
        &self.data[min(self.len(), lower_limit)..min(self.len(), upper_limit)]
    }

    pub fn header_size(&self) -> usize {
        match self.kind() {
            BinaryDataKind::VideoDataIframe => 32,
            BinaryDataKind::VideoDataPframe => 24,
            BinaryDataKind::AudioDataAac => 8,
            BinaryDataKind::AudioDataAdpcm => 16,
            BinaryDataKind::InfoData => 32,
            BinaryDataKind::Unknown | BinaryDataKind::Invalid | BinaryDataKind::Continue => 0,
        }
    }

    pub fn data_size(&self) -> usize {
        match self.kind() {
            BinaryDataKind::VideoDataIframe => BinaryData::bytes_to_size(&self.data[8..12]),
            BinaryDataKind::VideoDataPframe => BinaryData::bytes_to_size(&self.data[8..12]),
            BinaryDataKind::AudioDataAac => BinaryData::bytes_to_size(&self.data[4..6]),
            BinaryDataKind::AudioDataAdpcm => BinaryData::bytes_to_size(&self.data[4..6]),
            BinaryDataKind::InfoData => BinaryData::bytes_to_size(&self.data[4..8]),
            BinaryDataKind::Unknown | BinaryDataKind::Invalid | BinaryDataKind::Continue => {
                self.data.len()
            }
        }
    }

    fn bytes_to_size(bytes: &[u8]) -> usize {
        match bytes.len() {
            // 8 Won't fit into usize on a 32-bit machine
            4 => u32::from_le_bytes(bytes.try_into().expect("slice with incorrect length"))
                .try_into()
                .expect("u32 won't fit into usize"),
            2 => u16::from_le_bytes(bytes.try_into().expect("slice with incorrect length"))
                .try_into()
                .expect("u16 won't fit into usize"),
            1 => u8::from_le_bytes(bytes.try_into().expect("slice with incorrect length"))
                .try_into()
                .expect("u8 won't fit into usize"),
            _ => unreachable!(),
        }
    }

    pub fn kind(&self) -> BinaryDataKind {
        const MAGIC_VIDEO_INFO: &[u8] = &[0x31, 0x30, 0x30, 0x31];
        const MAGIC_AAC: &[u8] = &[0x30, 0x35, 0x77, 0x62];
        const MAGIC_ADPCM: &[u8] = &[0x30, 0x31, 0x77, 0x62];
        const MAGIC_IFRAME: &[u8] = &[0x30, 0x30, 0x64, 0x63];
        const MAGIC_PFRAME: &[u8] = &[0x30, 0x31, 0x64, 0x63];

        let magic = &self.data[..4];
        match magic {
            MAGIC_VIDEO_INFO => BinaryDataKind::InfoData,
            MAGIC_AAC => BinaryDataKind::AudioDataAac,
            MAGIC_ADPCM => BinaryDataKind::AudioDataAdpcm,
            MAGIC_IFRAME => BinaryDataKind::VideoDataIframe,
            MAGIC_PFRAME => BinaryDataKind::VideoDataPframe,
            _ if self.len() == CHUNK_SIZE => BinaryDataKind::Continue,
            _ => BinaryDataKind::Unknown,
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.data.as_slice()
    }
}

impl std::convert::AsRef<[u8]> for BinaryData {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ModernMsg {
    pub xml: Option<BcXml>,
    pub binary: Option<BinaryData>,
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

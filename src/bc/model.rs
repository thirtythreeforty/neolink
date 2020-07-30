use super::xml::BcXml;
use std::collections::HashSet;
use std::convert::TryInto;

pub(super) const MAGIC_HEADER: u32 = 0xabcdef0;

pub const MSG_ID_LOGIN: u32 = 1;
pub const MSG_ID_VIDEO: u32 = 3;
pub const MSG_ID_PING: u32 = 93;

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

pub const MAGIC_VIDEO_INFO: &[u8] = &[0x31, 0x30, 0x30, 0x31];
pub const MAGIC_AAC: &[u8] = &[0x30, 0x35, 0x77, 0x62];
pub const MAGIC_ADPCM: &[u8] = &[0x30, 0x31, 0x77, 0x62];
pub const MAGIC_IFRAME:  &[u8] = &[0x30, 0x30, 0x64, 0x63];
pub const MAGIC_PFRAME:  &[u8] = &[0x30, 0x31, 0x64, 0x63];

#[derive(Debug, PartialEq, Eq)]
pub struct VideoIFrame {
    pub raw: Vec<u8>
}

impl VideoIFrame {
    pub fn from_binary(binary: &[u8]) -> VideoIFrame {
        VideoIFrame {
            raw: binary.to_vec(),
        }
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.raw.as_slice()
    }

    pub fn magic(&self) -> &[u8] {
        &self.raw[0..4]
    }

    pub fn video_type(&self) -> &[u8] {
        &self.raw[4..8]
    }

    pub fn data_size(&self) -> usize {
        u32::from_be_bytes(self.raw[8..12].try_into().expect("slice with incorrect length")).try_into().expect("u32 won't fit into usize")
    }

    pub fn unknowna(&self) -> &[u8] {
        &self.raw[12..16]
    }

    pub fn timestamp(&self) -> &[u8] {
        &self.raw[16..20]
    }

    pub fn unknownb(&self) -> &[u8] {
        &self.raw[20..24]
    }

    pub fn clocktime(&self) -> &[u8] {
        &self.raw[24..28]
    }

    pub fn unknownc(&self) -> &[u8] {
        &self.raw[28..32]
    }

    pub fn video_data(&self) -> &[u8] {
        &self.raw[32..]
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct VideoPFrame {
    pub raw: Vec<u8>
}

impl VideoPFrame {
    pub fn from_binary(binary: &[u8]) -> VideoPFrame {
        VideoPFrame {
            raw: binary.to_vec(),
        }
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.raw.as_slice()
    }

    pub fn magic(&self) -> &[u8] {
        &self.raw[0..4]
    }

    pub fn video_type(&self) -> &[u8] {
        &self.raw[4..8]
    }

    pub fn data_size(&self) -> usize {
        u32::from_be_bytes(self.raw[8..12].try_into().expect("slice with incorrect length")).try_into().expect("u32 won't fit into usize")
    }

    pub fn unknowna(&self) -> &[u8] {
        &self.raw[12..16]
    }

    pub fn timestamp(&self) -> &[u8] {
        &self.raw[16..20]
    }

    pub fn unknownb(&self) -> &[u8] {
        &self.raw[20..24]
    }

    pub fn video_data(&self) -> &[u8] {
        &self.raw[24..]
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum VideoFrame {
    IFrame(VideoIFrame),
    PFrame(VideoPFrame),
}

impl VideoFrame {
    pub fn len(&self) -> usize {
        match self {
            VideoFrame::IFrame(binary) => binary.len(),
            VideoFrame::PFrame(binary) => binary.len(),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match self {
            VideoFrame::IFrame(binary) => binary.as_slice(),
            VideoFrame::PFrame(binary) => binary.as_slice(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BinaryData {
    VideoData(VideoFrame),
    AudioData(Vec<u8>),
    InfoData(Vec<u8>),
    Unknown(Vec<u8>),
}

// Used during serlisation to create the binary data
impl std::convert::AsRef<[u8]> for BinaryData {
    fn as_ref(&self) -> &[u8] {
        match self {
            BinaryData::VideoData(binary) => binary.as_slice(),
            BinaryData::AudioData(binary) => binary.as_slice(),
            BinaryData::InfoData(binary) => binary.as_slice(),
            BinaryData::Unknown(binary) => binary.as_slice(),
        }
    }
}

impl BinaryData {
    pub fn len(&self) -> usize {
        match self {
            BinaryData::VideoData(binary) => binary.len(),
            BinaryData::AudioData(binary) => binary.len(),
            BinaryData::InfoData(binary) => binary.len(),
            BinaryData::Unknown(binary) => binary.len(),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match self {
            BinaryData::VideoData(binary) => binary.as_slice(),
            BinaryData::AudioData(binary) => binary.as_slice(),
            BinaryData::InfoData(binary) => binary.as_slice(),
            BinaryData::Unknown(binary) => binary.as_slice(),
        }
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

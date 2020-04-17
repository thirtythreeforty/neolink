use super::xml::Body;

pub(super) const MAGIC_HEADER: u32 = 0xabcdef0;

const MSG_ID_LOGIN: u32 = 1;
const MSG_ID_VIDEO: u32 = 3;

pub struct Bc {
    pub(super) header: BcHeader,
    pub body: BcBody,
}

pub enum BcBody {
    LegacyMsg(LegacyMsg),
    ModernMsg(ModernMsg),
}

pub struct ModernMsg {
    pub xml: Option<Body>,
    pub binary: Option<Vec<u8>>,
}

pub enum LegacyMsg {
    LoginMsg {
        username: String,
        password: String,
    }
}

pub(super) struct BcHeader {
    pub(super) body_len: u32,
    pub(super) msg_id: u32,
    pub(super) enc_offset: u32,
    pub(super) encrypted: bool,
    pub(super) class: u16,
    pub(super) bin_offset: Option<u32>,
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
}


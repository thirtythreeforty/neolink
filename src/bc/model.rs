use nom::{IResult, error::ParseError};
use nom::{bytes::complete::take, number::complete::*, combinator::*, sequence::*};
use super::bc_xml::Body;
use super::xml_crypto;

const MAGIC_HEADER: u32 = 0xabcdef0;

const MSG_ID_LOGIN: u32 = 1;
const MSG_ID_VIDEO: u32 = 3;

pub struct Bc {
    header: BcHeader,
    body: BcBody,
}

pub enum BcBody {
    LegacyMsg(LegacyMsg),
    ModernMsg(ModernMsg),
}

pub struct ModernMsg {
    xml: Option<Body>,
    binary: Option<Vec<u8>>,
}

pub enum LegacyMsg {
    LoginMsg {
        username: String,
        password: String,
    }
}

struct BcHeader {
    body_len: u32,
    msg_id: u32,
    enc_offset: u32,
    encrypted: bool,
    class: u16,
    bin_offset: Option<u32>,
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

fn bc_msg(buf: &[u8]) -> IResult<&[u8], Bc> {
    let (buf, header) = bc_header(buf)?;
    let (buf, msg) = bc_modern_msg(&header, buf)?;

    let bc = Bc {
        header,
        body: BcBody::ModernMsg(msg),
    };

    Ok((buf, bc))
}

fn bc_modern_msg<'a, 'b, E: ParseError<&'b [u8]>>(header: &'a BcHeader, buf: &'b [u8]) -> IResult<&'b [u8], ModernMsg, E> {
    let end_of_xml = header.bin_offset.unwrap_or(buf.len() as u32);

    let (mut buf, body_buf) = take(end_of_xml)(buf)?;

    let decrypted;
    let processed_body_buf = if !header.is_encrypted() { buf } else {
        decrypted = xml_crypto::crypt(header.enc_offset, body_buf);
        &decrypted
    };

    // Apply the function, but throw away the reference to decrypted in the Ok and Err case
    // This error-error-error thing is the same idiom Nom uses internally.
    use nom::{Err, error::{make_error, ErrorKind}};
    let body = Body::try_parse(processed_body_buf)
        .map_err(|_| Err::Error(make_error(buf, ErrorKind::MapRes)))?;

    // Extract remainder of message as binary, if it exists
    let mut binary = None;
    if let Some(bin_offset) = header.bin_offset {
        let (buf_after, payload) = map(take(header.body_len - bin_offset), |x: &[u8]| x.to_vec())(buf)?;
        binary = Some(payload);
        buf = buf_after;
    }

    let msg = ModernMsg {
        xml: Some(body),
        binary,
    };

    Ok((buf, msg))
}

fn bc_header(buf: &[u8]) -> IResult<&[u8], BcHeader> {
    let (buf, _magic) = verify(le_u32, |x| *x == MAGIC_HEADER)(buf)?;
    let (buf, msg_id) = le_u32(buf)?;
    let (buf, body_len) = le_u32(buf)?;
    let (buf, enc_offset) = le_u32(buf)?;
    let (buf, (encrypted, _ignored, class)) = tuple((le_u8, le_u8, le_u16))(buf)?;
    let encrypted = encrypted != 0;

    // See BcHeader::is_modern() for a description of which packets have the bin offset
    let has_bin_offset = class == 0x6414 || class == 0x0000;
    let (buf, bin_offset) = cond(has_bin_offset, le_u32)(buf)?;

    Ok((buf, BcHeader {
        msg_id,
        body_len,
        enc_offset,
        encrypted,
        class,
        bin_offset,
    }))
}

#[test]
fn test_bc_modern_login() {
    let sample = include_bytes!("samples/model_sample_modern_login.bin");

    let (_, msg) = bc_msg(&sample[..]).unwrap();
    assert_eq!(msg.header.msg_id, 1);
    assert_eq!(msg.header.body_len, 145);
    assert_eq!(msg.header.enc_offset, 0x1000000);
    assert_eq!(msg.header.encrypted, true);
    assert_eq!(msg.header.class, 0x6614);
    match msg.body {
        BcBody::ModernMsg(ModernMsg{ xml: Some(xml), binary: None }) => {
            assert_eq!(xml.encryption.unwrap().nonce, "9E6D1FCB9E69846D")
        }
        _ => assert!(false),
    }
}

use nom::{IResult, error::ParseError};
use nom::{bytes::complete::take, number::complete::*, combinator::*, sequence::*};
use super::model::*;
use super::xml::Body;
use super::xml_crypto;

impl Bc {
    pub fn deserialize(buf: &[u8]) -> Result<Bc, ()> {
        // Throw away the nom-specific return types
        bc_msg(buf).map(|(_, bc)| bc).map_err(|_| ()) // TODO better error
    }
}

fn bc_msg(buf: &[u8]) -> IResult<&[u8], Bc> {
    let (buf, header) = bc_header(buf)?;
    let (buf, body) = bc_body(&header, buf)?;

    let bc = Bc {
        meta: header.to_meta(),
        body,
    };

    Ok((buf, bc))
}

fn bc_body<'a, 'b>(header: &'a BcHeader, buf: &'b [u8])
    -> IResult<&'b [u8], BcBody>
{
    if header.is_modern() {
        let (buf, body) = bc_modern_msg(header, buf)?;
        Ok((buf, BcBody::ModernMsg(body)))
    } else {
        unimplemented!()
    }
}

fn bc_modern_msg<'a, 'b, E: ParseError<&'b [u8]>>(header: &'a BcHeader, buf: &'b [u8])
    -> IResult<&'b [u8], ModernMsg, E>
{
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
    let (buf, (response_code, _ignored, class)) = tuple((le_u8, le_u8, le_u16))(buf)?;

    // All modern messages are encrypted.  In addition, it seems that the camera firmware checks
    // this field to see if some other messages should be encrypted.  This is still somewhat fuzzy.
    // A copy of the source code for the camera would be very useful.
    let encrypted = response_code != 0;

    let (buf, bin_offset) = cond(has_bin_offset(class), le_u32)(buf)?;

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

    let (buf, header) = bc_header(&sample[..]).unwrap();
    let (_, body) = bc_body(&header, buf).unwrap();
    assert_eq!(header.msg_id, 1);
    assert_eq!(header.body_len, 145);
    assert_eq!(header.enc_offset, 0x1000000);
    assert_eq!(header.encrypted, true);
    assert_eq!(header.class, 0x6614);
    match body {
        BcBody::ModernMsg(ModernMsg{ xml: Some(ref xml), binary: None }) => {
            assert_eq!(xml.encryption.as_ref().unwrap().nonce, "9E6D1FCB9E69846D")
        }
        _ => assert!(false),
    }
}

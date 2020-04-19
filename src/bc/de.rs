use err_derive::Error;
use nom::IResult;
use nom::{bytes::streaming::take, number::streaming::*, combinator::*, sequence::*};
use std::io::Read;
use super::model::*;
use super::xml::BcXml;
use super::xml_crypto;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display="Parsing error")]
    NomError(&'static str),
    #[error(display="I/O error")]
    IoError(#[error(source)] std::io::Error),
}

type NomErrorTuple<'a> = (&'a [u8], nom::error::ErrorKind);

impl<'a> From<nom::Err<NomErrorTuple<'a>>> for Error {
    fn from(k: nom::Err<NomErrorTuple<'a>>) -> Self {
        let reason = match k {
            nom::Err::Error(_) => "Nom Error",
            nom::Err::Failure(_) => "Nom Failure",
            _ => "Unknown Nom error",
        };
        Error::NomError(reason)
    }
}

impl Bc {
    pub fn deserialize<R: Read>(r: R) -> Result<Bc, Error> {
        // Throw away the nom-specific return types
        read_from_reader(bc_msg, r)
    }
}

fn read_from_reader<P, O, E, R>(parser: P, mut rdr: R) -> Result<O, E>
    where R: Read,
          E: for<'a> From<nom::Err<NomErrorTuple<'a>>> + From<std::io::Error>,
          P: Fn(&[u8]) -> nom::IResult<&[u8], O>,
{
    let mut input: Vec<u8> = Vec::new();
    loop {
        let to_read = match parser(&input) {
            Ok((_, parsed)) => return Ok(parsed),
            Err(nom::Err::Incomplete(needed)) => {
                match needed {
                    nom::Needed::Unknown => 1,     // read one byte
                    nom::Needed::Size(len) => len,
                }
            },
            Err(e) => return Err(e.into()),
        };

        (&mut rdr).take(to_read as u64).read_to_end(&mut input)?;
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
        let (buf, body) = match header.msg_id {
            MSG_ID_LOGIN => bc_legacy_login_msg(buf)?,
            _ => (buf, LegacyMsg::UnknownMsg),
        };
        Ok((buf, BcBody::LegacyMsg(body)))
    }
}

fn hex32<'a>() -> impl Fn(&'a [u8]) -> IResult<&'a [u8], String> {
    map_res(take(32usize), |slice: &'a [u8]| String::from_utf8(slice.to_vec()))
}

fn bc_legacy_login_msg<'a, 'b>(buf: &'b [u8])
    -> IResult<&'b [u8], LegacyMsg>
{
    let (buf, username) = hex32()(buf)?;
    let (buf, password) = hex32()(buf)?;

    Ok((buf, LegacyMsg::LoginMsg {
        username,
        password,
    }))
}

fn bc_modern_msg<'a, 'b>(header: &'a BcHeader, buf: &'b [u8])
    -> IResult<&'b [u8], ModernMsg>
{
    let end_of_xml = match header.bin_offset {
        Some(off) if off > 0 => off,
        _ => header.body_len
    };

    let (mut buf, body_buf) = take(end_of_xml)(buf)?;

    let decrypted;
    let processed_body_buf = if !header.is_encrypted() { buf } else {
        decrypted = xml_crypto::crypt(header.enc_offset, body_buf);
        &decrypted
    };

    // Extract remainder of message as binary, if it exists
    let mut binary = None;
    if let Some(bin_offset) = header.bin_offset {
        if bin_offset > 0 {
            let (buf_after, payload) = map(take(header.body_len - bin_offset),
                                           |x: &[u8]| x.to_vec())(buf)?;
            binary = Some(payload);
            buf = buf_after;
        }
    }

    // Now we'll take the buffer that Nom gave a ref to and parse it.  Do this last, so we don't
    // parse over and over if we get Incompletes on the binary part.  Apply the XML parse function,
    // but throw away the reference to decrypted in the Ok and Err case This error-error-error
    // thing is the same idiom Nom uses internally.
    use nom::{Err, error::{make_error, ErrorKind}};
    let xml = if end_of_xml > 0 {
        let parsed = BcXml::try_parse(processed_body_buf)
            .map_err(|_| Err::Error(make_error(buf, ErrorKind::MapRes)))?;
        Some(parsed)
    } else { None };

    let msg = ModernMsg { xml, binary, };

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

#[test]
fn test_bc_legacy_login() {
    let sample = include_bytes!("samples/model_sample_legacy_login.bin");

    let (buf, header) = bc_header(&sample[..]).unwrap();
    let (_, body) = bc_body(&header, buf).unwrap();
    assert_eq!(header.msg_id, 1);
    assert_eq!(header.body_len, 1836);
    assert_eq!(header.enc_offset, 0x1000000);
    assert_eq!(header.encrypted, true);
    assert_eq!(header.class, 0x6514);
    match body {
        BcBody::LegacyMsg(LegacyMsg::LoginMsg {
            username, password
        }) => {
            assert_eq!(username, "21232F297A57A5A743894A0E4A801FC\0");
            assert_eq!(password, EMPTY_LEGACY_PASSWORD);
        }
        _ => assert!(false),
    }
}

#[test]
fn test_bc_modern_login_failed() {
    let sample = include_bytes!("samples/modern_login_failed.bin");

    let (buf, header) = bc_header(&sample[..]).unwrap();
    let (_, body) = bc_body(&header, buf).unwrap();
    assert_eq!(header.msg_id, 1);
    assert_eq!(header.body_len, 0);
    assert_eq!(header.enc_offset, 0x0);
    assert_eq!(header.encrypted, true);
    assert_eq!(header.class, 0x0000);
    match body {
        BcBody::ModernMsg(ModernMsg{ xml: None, binary: None }) => {
            assert!(true);
        }
        _ => assert!(false),
    }
}

#[test]
fn test_bc_modern_login_success() {
    let sample = include_bytes!("samples/modern_login_success.bin");

    let (buf, header) = bc_header(&sample[..]).unwrap();
    let (_, body) = bc_body(&header, buf).unwrap();
    assert_eq!(header.msg_id, 1);
    assert_eq!(header.body_len, 2949);
    assert_eq!(header.enc_offset, 0x0);
    assert_eq!(header.encrypted, true);
    assert_eq!(header.class, 0x0000);

    // Previously, we were not handling bin_offset == 0 (no bin offset) correctly.
    // Test that we decoded XML and no binary.
    match body {
        BcBody::ModernMsg(ModernMsg{ xml: Some(_), binary: None }) => assert!(true),
        _ => assert!(false),
    }
}

use super::model::BcMediaIframe;
use super::model::*;
use super::xml::{BcPayloads, BcXml};
use super::xml_crypto;
use err_derive::Error;
use log::*;
use nom::IResult;
use nom::{
    bytes::streaming::take, combinator::*, number::streaming::*, sequence::*, take, take_str,
};
use std::io::Read;

// PAD_SIZE: Media packets use 8 byte padding
const PAD_SIZE: u32 = 8;

/// The error types used during deserialisation
#[derive(Debug, Error)]
pub enum Error {
    /// A Nom parsing error usually a malformed packet
    #[error(display = "Parsing error")]
    NomError(String),
    /// An IO error such as the stream being dropped
    #[error(display = "I/O error")]
    IoError(#[error(source)] std::io::Error),
}

type NomErrorType<'a> = nom::error::Error<&'a [u8]>;

impl<'a> From<nom::Err<NomErrorType<'a>>> for Error {
    fn from(k: nom::Err<NomErrorType<'a>>) -> Self {
        let reason = match k {
            nom::Err::Error(e) => format!("Nom Error: {:?}", e),
            nom::Err::Failure(e) => format!("Nom Error: {:?}", e),
            _ => "Unknown Nom error".to_string(),
        };
        Error::NomError(reason)
    }
}

impl Bc {
    pub(crate) fn deserialize<R: Read>(context: &mut BcContext, r: R) -> Result<Bc, Error> {
        // Throw away the nom-specific return types
        read_from_reader(|reader| bc_msg(context, reader), r)
    }
}

fn read_from_reader<P, O, E, R>(mut parser: P, mut rdr: R) -> Result<O, E>
where
    R: Read,
    E: for<'a> From<nom::Err<NomErrorType<'a>>> + From<std::io::Error>,
    P: FnMut(&[u8]) -> nom::IResult<&[u8], O>,
{
    let mut input: Vec<u8> = Vec::new();
    loop {
        let to_read = match parser(&input) {
            Ok((_, parsed)) => return Ok(parsed),
            Err(nom::Err::Incomplete(needed)) => {
                match needed {
                    nom::Needed::Unknown => std::num::NonZeroUsize::new(1).unwrap(), // read one byte
                    nom::Needed::Size(len) => len,
                }
            }
            Err(e) => return Err(e.into()),
        };

        if 0 == (&mut rdr)
            .take(to_read.get() as u64)
            .read_to_end(&mut input)?
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Read returned 0 bytes",
            )
            .into());
        }
    }
}

fn bc_msg<'a, 'b>(context: &'a mut BcContext, buf: &'b [u8]) -> IResult<&'b [u8], Bc> {
    let (buf, header) = bc_header(buf)?;
    let (buf, body) = bc_body(context, &header, buf)?;

    let bc = Bc {
        meta: header.to_meta(),
        body,
    };

    Ok((buf, bc))
}

fn bc_body<'a, 'b, 'c>(
    context: &'c mut BcContext,
    header: &'a BcHeader,
    buf: &'b [u8],
) -> IResult<&'b [u8], BcBody> {
    if header.is_modern() {
        let (buf, body) = bc_modern_msg(context, header, buf)?;
        Ok((buf, BcBody::ModernMsg(body)))
    } else {
        let (buf, body) = match header.msg_id {
            MSG_ID_LOGIN => bc_legacy_login_msg(buf)?,
            _ => (buf, LegacyMsg::UnknownMsg),
        };
        Ok((buf, BcBody::LegacyMsg(body)))
    }
}

fn hex32<'a>() -> impl FnMut(&'a [u8]) -> IResult<&'a [u8], String> {
    map_res(take(32usize), |slice: &'a [u8]| {
        String::from_utf8(slice.to_vec())
    })
}

fn bc_legacy_login_msg(buf: &'_ [u8]) -> IResult<&'_ [u8], LegacyMsg> {
    let (buf, username) = hex32()(buf)?;
    let (buf, password) = hex32()(buf)?;

    Ok((buf, LegacyMsg::LoginMsg { username, password }))
}

fn bc_modern_msg<'a, 'b>(
    context: &mut BcContext,
    header: &'a BcHeader,
    buf: &'b [u8],
) -> IResult<&'b [u8], ModernMsg> {
    use nom::{
        error::{make_error, ErrorKind},
        Err,
    };

    let ext_len = match header.payload_offset {
        Some(off) => off,
        _ => 0, // If missing payload_offset treat all as payload
    };

    if header.msg_id == 1 && (header.response_code >> 8) == 0xdd {
        // Login reply has the encryption info
        // Set that the encryption type now
        let encryption_protocol_byte = (header.response_code & 0xff) as usize;
        match encryption_protocol_byte {
            0x00 => context.set_encrypted(EncryptionProtocol::Unencrypted),
            0x01 => context.set_encrypted(EncryptionProtocol::BCEncrypt),
            0x02 => context.set_encrypted(EncryptionProtocol::Aes(None)),
            _ => return Err(Err::Error(make_error(buf, ErrorKind::MapRes))),
        }
    }

    let (buf, ext_buf) = take(ext_len)(buf)?;
    let payload_len = header.body_len - ext_len;
    let (buf, payload_buf) = take(payload_len)(buf)?;

    let decrypted;
    let processed_ext_buf = match context.get_encrypted() {
        EncryptionProtocol::Unencrypted => ext_buf,
        encryption_protocol => {
            decrypted =
                xml_crypto::decrypt(header.channel_id as u32, ext_buf, &encryption_protocol);
            &decrypted
        }
    };

    // Now we'll take the buffer that Nom gave a ref to and parse it.
    let extension;
    if ext_len > 0 {
        // Apply the XML parse function, but throw away the reference to decrypted in the Ok and
        // Err case. This error-error-error thing is the same idiom Nom uses internally.
        let parsed = Extension::try_parse(processed_ext_buf)
            .map_err(|_| Err::Error(make_error(buf, ErrorKind::MapRes)))?;
        if let Extension {
            binary_data: Some(1),
            ..
        } = &parsed
        {
            context.in_bin_mode.insert(header.msg_num);
        }
        extension = Some(parsed);
    } else {
        extension = None;
    }

    // Now to handle the payload block
    // This block can either be xml or binary depending on what the message expects.
    // For our purposes we use try_parse and if all xml based parsers fail we treat
    // As binary
    let payload;
    if payload_len > 0 {
        // Extract remainder of message as binary, if it exists
        let encryption_protocol = context.get_encrypted();
        let processed_payload_buf =
            xml_crypto::decrypt(header.channel_id as u32, payload_buf, &encryption_protocol);
        if context.in_bin_mode.contains(&(header.msg_num)) {
            payload = Some(BcPayloads::Binary(payload_buf.to_vec()));
        } else {
            let xml = BcXml::try_parse(processed_payload_buf.as_slice())
                .map_err(|_| Err::Error(make_error(buf, ErrorKind::MapRes)))?;
            payload = Some(BcPayloads::BcXml(xml));
        }
    } else {
        payload = None;
    }

    Ok((buf, ModernMsg { extension, payload }))
}

fn bc_header(buf: &[u8]) -> IResult<&[u8], BcHeader> {
    let (buf, _magic) = verify(le_u32, |x| *x == MAGIC_HEADER)(buf)?;
    let (buf, msg_id) = le_u32(buf)?;
    let (buf, body_len) = le_u32(buf)?;
    let (buf, channel_id) = le_u8(buf)?;
    let (buf, stream_type) = le_u8(buf)?;
    let (buf, msg_num) = le_u16(buf)?;
    let (buf, (response_code, class)) = tuple((le_u16, le_u16))(buf)?;

    let (buf, payload_offset) = cond(has_payload_offset(class), le_u32)(buf)?;

    Ok((
        buf,
        BcHeader {
            body_len,
            msg_id,
            channel_id,
            stream_type,
            msg_num,
            response_code,
            class,
            payload_offset,
        },
    ))
}

impl BcMedia {
    pub(crate) fn deserialize<R: Read>(r: R) -> Result<BcMedia, Error> {
        // Throw away the nom-specific return types
        read_from_reader(|reader| bcmedia(reader), r)
    }
}

fn bcmedia(buf: &[u8]) -> IResult<&[u8], BcMedia> {
    let (buf, magic) = verify(le_u32, |x| {
        matches!(
            *x,
            MAGIC_HEADER_BCMEDIA_INFO_V1
                | MAGIC_HEADER_BCMEDIA_INFO_V2
                | MAGIC_HEADER_BCMEDIA_IFRAME
                | MAGIC_HEADER_BCMEDIA_PFRAME
                | MAGIC_HEADER_BCMEDIA_AAC
                | MAGIC_HEADER_BCMEDIA_ADPCM
        )
    })(buf)?;

    match magic {
        MAGIC_HEADER_BCMEDIA_INFO_V1 => {
            let (buf, payload) = bcmedia_info_v1(buf)?;
            Ok((buf, BcMedia::InfoV1(payload)))
        }
        MAGIC_HEADER_BCMEDIA_INFO_V2 => {
            let (buf, payload) = bcmedia_info_v2(buf)?;
            Ok((buf, BcMedia::InfoV2(payload)))
        }
        MAGIC_HEADER_BCMEDIA_IFRAME => {
            let (buf, payload) = bcmedia_iframe(buf)?;
            Ok((buf, BcMedia::Iframe(payload)))
        }
        MAGIC_HEADER_BCMEDIA_PFRAME => {
            let (buf, payload) = bcmedia_pframe(buf)?;
            Ok((buf, BcMedia::Pframe(payload)))
        }
        MAGIC_HEADER_BCMEDIA_AAC => {
            let (buf, payload) = bcmedia_aac(buf)?;
            Ok((buf, BcMedia::Aac(payload)))
        }
        MAGIC_HEADER_BCMEDIA_ADPCM => {
            let (buf, payload) = bcmedia_adpcm(buf)?;
            Ok((buf, BcMedia::Adpcm(payload)))
        }
        _ => unreachable!(),
    }
}

fn bcmedia_info_v1(buf: &[u8]) -> IResult<&[u8], BcMediaInfoV1> {
    let (buf, header_size) = verify(le_u32, |x| *x == 32)(buf)?;
    let (buf, video_width) = le_u32(buf)?;
    let (buf, video_height) = le_u32(buf)?;
    let (buf, _unknown) = le_u8(buf)?;
    let (buf, fps) = le_u8(buf)?;
    let (buf, start_year) = le_u8(buf)?;
    let (buf, start_month) = le_u8(buf)?;
    let (buf, start_day) = le_u8(buf)?;
    let (buf, start_hour) = le_u8(buf)?;
    let (buf, start_min) = le_u8(buf)?;
    let (buf, start_seconds) = le_u8(buf)?;
    let (buf, end_year) = le_u8(buf)?;
    let (buf, end_month) = le_u8(buf)?;
    let (buf, end_day) = le_u8(buf)?;
    let (buf, end_hour) = le_u8(buf)?;
    let (buf, end_min) = le_u8(buf)?;
    let (buf, end_seconds) = le_u8(buf)?;
    let (buf, _unknown_b) = le_u16(buf)?;

    Ok((
        buf,
        BcMediaInfoV1 {
            header_size,
            video_width,
            video_height,
            fps,
            start_year,
            start_month,
            start_day,
            start_hour,
            start_min,
            start_seconds,
            end_year,
            end_month,
            end_day,
            end_hour,
            end_min,
            end_seconds,
        },
    ))
}

fn bcmedia_info_v2(buf: &[u8]) -> IResult<&[u8], BcMediaInfoV2> {
    let (buf, header_size) = verify(le_u32, |x| *x == 32)(buf)?;
    let (buf, video_width) = le_u32(buf)?;
    let (buf, video_height) = le_u32(buf)?;
    let (buf, _unknown) = le_u8(buf)?;
    let (buf, fps) = le_u8(buf)?;
    let (buf, start_year) = le_u8(buf)?;
    let (buf, start_month) = le_u8(buf)?;
    let (buf, start_day) = le_u8(buf)?;
    let (buf, start_hour) = le_u8(buf)?;
    let (buf, start_min) = le_u8(buf)?;
    let (buf, start_seconds) = le_u8(buf)?;
    let (buf, end_year) = le_u8(buf)?;
    let (buf, end_month) = le_u8(buf)?;
    let (buf, end_day) = le_u8(buf)?;
    let (buf, end_hour) = le_u8(buf)?;
    let (buf, end_min) = le_u8(buf)?;
    let (buf, end_seconds) = le_u8(buf)?;
    let (buf, _unknown_b) = le_u16(buf)?;

    Ok((
        buf,
        BcMediaInfoV2 {
            header_size,
            video_width,
            video_height,
            fps,
            start_year,
            start_month,
            start_day,
            start_hour,
            start_min,
            start_seconds,
            end_year,
            end_month,
            end_day,
            end_hour,
            end_min,
            end_seconds,
        },
    ))
}

fn bcmedia_iframe(buf: &[u8]) -> IResult<&[u8], BcMediaIframe> {
    let (buf, video_type_str) = take_str!(buf, 4)?;
    let (buf, payload_size) = le_u32(buf)?;
    let (buf, _unknown_a) = le_u32(buf)?;
    let (buf, microseconds) = le_u32(buf)?;
    let (buf, _unknown_b) = le_u32(buf)?;
    let (buf, time) = le_u32(buf)?;
    let (buf, _unknown_c) = le_u32(buf)?;
    let (buf, data_slice) = take!(buf, payload_size)?;
    let pad_size = match payload_size % PAD_SIZE {
        0 => 0,
        n => PAD_SIZE - n,
    };
    let (buf, _padding) = take!(buf, pad_size)?;

    Ok((
        buf,
        BcMediaIframe {
            video_type: video_type_str.to_string(),
            payload_size,
            microseconds,
            time,
            data: data_slice.to_vec(),
        },
    ))
}

fn bcmedia_pframe(buf: &[u8]) -> IResult<&[u8], BcMediaPframe> {
    let (buf, video_type_str) = take_str!(buf, 4)?;
    let (buf, payload_size) = le_u32(buf)?;
    let (buf, _unknown_a) = le_u32(buf)?;
    let (buf, microseconds) = le_u32(buf)?;
    let (buf, _unknown_b) = le_u32(buf)?;
    let (buf, data_slice) = take!(buf, payload_size)?;
    let pad_size = match payload_size % PAD_SIZE {
        0 => 0,
        n => PAD_SIZE - n,
    };
    let (buf, _padding) = take!(buf, pad_size)?;

    Ok((
        buf,
        BcMediaPframe {
            video_type: video_type_str.to_string(),
            payload_size,
            microseconds,
            data: data_slice.to_vec(),
        },
    ))
}

fn bcmedia_aac(buf: &[u8]) -> IResult<&[u8], BcMediaAac> {
    let (buf, payload_size) = le_u16(buf)?;
    let (buf, _payload_size_b) = le_u16(buf)?;
    let (buf, data_slice) = take!(buf, payload_size)?;
    let pad_size = match payload_size as u32 % PAD_SIZE {
        0 => 0,
        n => PAD_SIZE - n,
    };
    let (buf, _padding) = take!(buf, pad_size)?;

    Ok((
        buf,
        BcMediaAac {
            payload_size,
            data: data_slice.to_vec(),
        },
    ))
}

fn bcmedia_adpcm(buf: &[u8]) -> IResult<&[u8], BcMediaAdpcm> {
    let (buf, payload_size) = le_u16(buf)?;
    let (buf, _payload_size_b) = le_u16(buf)?;
    let (buf, _magic) = verify(le_u16, |x| *x == MAGIC_HEADER_BCMEDIA_ADPCM_DATA)(buf)?;
    let (buf, block_size) = le_u16(buf)?;
    let (buf, data_slice) = take!(buf, block_size)?;
    let pad_size = match payload_size as u32 % PAD_SIZE {
        0 => 0,
        n => PAD_SIZE - n,
    };
    let (buf, _padding) = take!(buf, pad_size)?;

    Ok((
        buf,
        BcMediaAdpcm {
            payload_size,
            block_size,
            data: data_slice.to_vec(),
        },
    ))
}

#[test]
fn test_bc_modern_login() {
    let sample = include_bytes!("samples/model_sample_modern_login.bin");

    let encryption_protocol =
        std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
    let mut context = BcContext::new(encryption_protocol);

    let (buf, header) = bc_header(&sample[..]).unwrap();
    let (_, body) = bc_body(&mut context, &header, buf).unwrap();
    assert_eq!(header.msg_id, 1);
    assert_eq!(header.body_len, 145);
    assert_eq!(header.channel_id, 0);
    assert_eq!(header.stream_type, 0);
    assert_eq!(header.payload_offset, None);
    assert_eq!(header.response_code, 0xdd01);
    assert_eq!(header.class, 0x6614);
    match body {
        BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    encryption: Some(encryption),
                    ..
                })),
            ..
        }) => assert_eq!(encryption.nonce, "9E6D1FCB9E69846D"),
        _ => panic!(),
    }
}

#[test]
fn test_bc_legacy_login() {
    let sample = include_bytes!("samples/model_sample_legacy_login.bin");

    let encryption_protocol =
        std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
    let mut context = BcContext::new(encryption_protocol);

    let (buf, header) = bc_header(&sample[..]).unwrap();
    let (_, body) = bc_body(&mut context, &header, buf).unwrap();
    assert_eq!(header.msg_id, 1);
    assert_eq!(header.body_len, 1836);
    assert_eq!(header.channel_id, 0);
    assert_eq!(header.stream_type, 0);
    assert_eq!(header.payload_offset, None);
    assert_eq!(header.response_code, 0xdc01);
    assert_eq!(header.class, 0x6514);
    match body {
        BcBody::LegacyMsg(LegacyMsg::LoginMsg { username, password }) => {
            assert_eq!(username, "21232F297A57A5A743894A0E4A801FC\0");
            assert_eq!(password, EMPTY_LEGACY_PASSWORD);
        }
        _ => panic!(),
    }
}

#[test]
fn test_bc_modern_login_failed() {
    let sample = include_bytes!("samples/modern_login_failed.bin");

    let encryption_protocol =
        std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
    let mut context = BcContext::new(encryption_protocol);

    let (buf, header) = bc_header(&sample[..]).unwrap();
    let (_, body) = bc_body(&mut context, &header, buf).unwrap();
    assert_eq!(header.msg_id, 1);
    assert_eq!(header.body_len, 0);
    assert_eq!(header.channel_id, 0);
    assert_eq!(header.stream_type, 0);
    assert_eq!(header.payload_offset, Some(0x0));
    assert_eq!(header.response_code, 0x190); // 400
    assert_eq!(header.class, 0x0000);
    match body {
        BcBody::ModernMsg(ModernMsg {
            extension: None,
            payload: None,
        }) => {}
        _ => panic!(),
    }
}

#[test]
fn test_bc_modern_login_success() {
    let sample = include_bytes!("samples/modern_login_success.bin");

    let encryption_protocol =
        std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
    let mut context = BcContext::new(encryption_protocol);

    let (buf, header) = bc_header(&sample[..]).unwrap();
    let (_, body) = bc_body(&mut context, &header, buf).unwrap();
    assert_eq!(header.msg_id, 1);
    assert_eq!(header.body_len, 2949);
    assert_eq!(header.channel_id, 0);
    assert_eq!(header.stream_type, 0);
    assert_eq!(header.payload_offset, Some(0x0));
    assert_eq!(header.response_code, 0xc8); // 200
    assert_eq!(header.class, 0x0000);

    // Previously, we were not handling payload_offset == 0 (no bin offset) correctly.
    // Test that we decoded XML and no binary.
    match body {
        BcBody::ModernMsg(ModernMsg {
            extension: None,
            payload: Some(_),
        }) => {}
        _ => panic!(),
    }
}

#[test]
fn test_bc_binary_mode() {
    let sample1 = include_bytes!("samples/modern_video_start1.bin");
    let sample2 = include_bytes!("samples/modern_video_start2.bin");

    let encryption_protocol =
        std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
    let mut context = BcContext::new(encryption_protocol);

    let msg1 = Bc::deserialize(&mut context, &sample1[..]).unwrap();
    match msg1.body {
        BcBody::ModernMsg(ModernMsg {
            extension:
                Some(Extension {
                    binary_data: Some(1),
                    ..
                }),
            payload: Some(BcPayloads::Binary(bin)),
        }) => {
            assert_eq!(bin.len(), 32);
        }
        _ => panic!(),
    }

    context.in_bin_mode.insert(msg1.meta.msg_num);
    let msg2 = Bc::deserialize(&mut context, &sample2[..]).unwrap();
    match msg2.body {
        BcBody::ModernMsg(ModernMsg {
            extension: None,
            payload: Some(BcPayloads::Binary(bin)),
        }) => {
            assert_eq!(bin.len(), 30344);
        }
        _ => panic!(),
    }
}

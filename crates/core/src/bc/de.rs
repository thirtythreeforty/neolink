use super::model::*;
use super::xml::{BcPayloads, BcXml};
use super::xml_crypto;
use crate::RX_TIMEOUT;
use err_derive::Error;
use nom::{
    bytes::streaming::take, combinator::*, error::context as error_context, number::streaming::*,
    sequence::*,
};
use std::io::Read;
use time::OffsetDateTime;

type IResult<I, O, E = nom::error::VerboseError<I>> = Result<(I, O), nom::Err<E>>;

/// The error types used during deserialisation
#[derive(Debug, Error, Clone)]
pub enum Error {
    /// A Nom parsing error usually a malformed packet
    #[error(display = "Parsing error: {}", _0)]
    NomError(String),
    /// An IO error such as the stream being dropped
    #[error(display = "I/O error")]
    IoError(#[error(source)] std::sync::Arc<std::io::Error>),
}
type NomErrorType<'a> = nom::error::VerboseError<&'a [u8]>;

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

impl From<std::io::Error> for Error {
    fn from(k: std::io::Error) -> Self {
        Error::IoError(std::sync::Arc::new(k))
    }
}

fn read_from_reader<P, O, E, R>(mut parser: P, mut rdr: R) -> Result<O, E>
where
    R: Read,
    E: for<'a> From<nom::Err<NomErrorType<'a>>> + From<std::io::Error>,
    P: FnMut(&[u8]) -> IResult<&[u8], O>,
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

        let start_time = OffsetDateTime::now_utc();
        loop {
            match (&mut rdr)
                .take(to_read.get() as u64)
                .read_to_end(&mut input)
            {
                Ok(0) => {
                    if (OffsetDateTime::now_utc() - start_time) > RX_TIMEOUT {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "Read returned 0 bytes",
                        )
                        .into());
                    }
                }
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // This is a temporaily unavaliable resource
                    // We should just wait and try again
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
    }
}

impl Bc {
    pub(crate) fn deserialize<R: Read>(context: &mut BcContext, r: R) -> Result<Bc, Error> {
        // Throw away the nom-specific return types
        read_from_reader(|reader| bc_msg(context, reader), r)
    }
}

fn bc_msg<'a>(context: &mut BcContext, buf: &'a [u8]) -> IResult<&'a [u8], Bc> {
    let (buf, header) = bc_header(buf)?;
    let (buf, body) = bc_body(context, &header, buf)?;

    let bc = Bc {
        meta: header.to_meta(),
        body,
    };

    Ok((buf, bc))
}

fn bc_body<'a>(
    context: &mut BcContext,
    header: &BcHeader,
    buf: &'a [u8],
) -> IResult<&'a [u8], BcBody> {
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

fn bc_modern_msg<'a>(
    context: &mut BcContext,
    header: &BcHeader,
    buf: &'a [u8],
) -> IResult<&'a [u8], ModernMsg> {
    use nom::{
        error::{ContextError, ErrorKind, ParseError},
        Err,
    };

    fn make_error<I, E: ParseError<I>>(input: I, ctx: &'static str, kind: ErrorKind) -> E
    where
        I: std::marker::Copy,
        E: ContextError<I>,
    {
        E::add_context(input, ctx, E::from_error_kind(input, kind))
    }

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
            _ => {
                return Err(Err::Error(make_error(
                    buf,
                    "Encryption Protocol is Unknown",
                    ErrorKind::MapRes,
                )));
            }
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
    let extension = if ext_len > 0 {
        // Apply the XML parse function, but throw away the reference to decrypted in the Ok and
        // Err case. This error-error-error thing is the same idiom Nom uses internally.
        let parsed = Extension::try_parse(processed_ext_buf).map_err(|_| {
            Err::Error(make_error(
                buf,
                "Unable to parse Extension XML",
                ErrorKind::MapRes,
            ))
        })?;
        if let Extension {
            binary_data: Some(1),
            ..
        } = &parsed
        {
            context.in_bin_mode.insert(header.msg_num);
        }
        Some(parsed)
    } else {
        None
    };

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
            let xml = BcXml::try_parse(processed_payload_buf.as_slice()).map_err(|_| {
                Err::Error(make_error(
                    buf,
                    "Unable to parse Payload XML",
                    ErrorKind::MapRes,
                ))
            })?;
            payload = Some(BcPayloads::BcXml(xml));
        }
    } else {
        payload = None;
    }

    Ok((buf, ModernMsg { extension, payload }))
}

fn bc_header(buf: &[u8]) -> IResult<&[u8], BcHeader> {
    let (buf, _magic) =
        error_context("Magic invalid", verify(le_u32, |x| *x == MAGIC_HEADER))(buf)?;
    let (buf, msg_id) = error_context("MsgID missing", le_u32)(buf)?;
    let (buf, body_len) = error_context("BodyLen missing", le_u32)(buf)?;
    let (buf, channel_id) = error_context("ChannelID missing", le_u8)(buf)?;
    let (buf, stream_type) = error_context("StreamType missing", le_u8)(buf)?;
    let (buf, msg_num) = error_context("MsgNum missing", le_u16)(buf)?;
    let (buf, (response_code, class)) =
        error_context("ResponseCode missing", tuple((le_u16, le_u16)))(buf)?;

    let (buf, payload_offset) = error_context(
        "Payload Offset is missing",
        cond(has_payload_offset(class), le_u32),
    )(buf)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bc::xml::*;
    use assert_matches::assert_matches;

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
    // This is an 0xdd03 encryption from an Argus 2
    //
    // It is currently unsupported
    fn test_03_enc_login() {
        let sample = include_bytes!("samples/battery_enc.bin");

        let encryption_protocol =
            std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
        let mut context = BcContext::new(encryption_protocol);

        let (buf, header) = bc_header(&sample[..]).unwrap();
        assert!(bc_body(&mut context, &header, buf).is_err());
        // It should error because we don't support it
        //
        // The following would be its contents if we
        // did support it (maybe one day :) left it here
        // for then)
        //
        //
        // let (_, body) = bc_body(&mut context, &header, buf).unwrap();
        // assert_eq!(header.msg_id, 1);
        // assert_eq!(header.body_len, 175);
        // assert_eq!(header.channel_id, 0);
        // assert_eq!(header.stream_type, 0);
        // assert_eq!(header.payload_offset, None);
        // assert_eq!(header.response_code, 0xdd03);
        // assert_eq!(header.class, 0x6614);
        // match body {
        //     BcBody::ModernMsg(ModernMsg {
        //         payload:
        //             Some(BcPayloads::BcXml(BcXml {
        //                 encryption: Some(encryption),
        //                 ..
        //             })),
        //         ..
        //     }) => assert_eq!(encryption.nonce, "0-AhnEZyUg6eKrJFIWgXPF"),
        //     _ => panic!(),
        // }
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

    #[test]
    // B800 seems to have a different header to the E1 and swann cameras
    // the stream_type and message_num do not seem to set in the offical clients
    //
    // They also have extra streams
    fn test_bc_b800_externstream() {
        let sample = include_bytes!("samples/xml_externstream_b800.bin");

        let encryption_protocol =
            std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
        let mut context = BcContext::new(encryption_protocol);

        let e = Bc::deserialize(&mut context, &sample[..]);
        assert_matches!(
            e,
            Ok(Bc {
                meta:
                    BcMeta {
                        msg_id: 3,
                        channel_id: 0x8c,
                        stream_type: 0,
                        response_code: 0,
                        msg_num: 0,
                        class: 0x6414,
                    },
                body:
                    BcBody::ModernMsg(ModernMsg {
                        extension: None,
                        payload:
                            Some(BcPayloads::BcXml(BcXml {
                                preview:
                                    Some(Preview {
                                        version,
                                        channel_id: 0,
                                        handle: 1024,
                                        stream_type,
                                    }),
                                ..
                            })),
                    }),
            }) if version == "1.1" && stream_type == Some("externStream".to_string())
        );
    }

    #[test]
    // B800 seems to have a different header to the E1 and swann cameras
    // the stream_type and message_num do not seem to set in the offical clients
    //
    // They also have extra streams
    fn test_bc_b800_substream() {
        let sample = include_bytes!("samples/xml_substream_b800.bin");

        let encryption_protocol =
            std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
        let mut context = BcContext::new(encryption_protocol);

        let e = Bc::deserialize(&mut context, &sample[..]);
        assert_matches!(
            e,
            Ok(Bc {
                meta:
                    BcMeta {
                        msg_id: 3,
                        channel_id: 143,
                        stream_type: 0,
                        response_code: 0,
                        msg_num: 0,
                        class: 0x6414,
                    },
                body:
                    BcBody::ModernMsg(ModernMsg {
                        extension: None,
                        payload:
                            Some(BcPayloads::BcXml(BcXml {
                                preview:
                                    Some(Preview {
                                        version,
                                        channel_id: 0,
                                        handle: 256,
                                        stream_type,
                                    }),
                                ..
                            })),
                    }),
            }) if version == "1.1" && stream_type == Some("subStream".to_string())
        );
    }

    #[test]
    // B800 seems to have a different header to the E1 and swann cameras
    // the stream_type and message_num do not seem to set in the offical clients
    //
    // They also have extra streams
    fn test_bc_b800_mainstream() {
        let sample = include_bytes!("samples/xml_mainstream_b800.bin");

        let encryption_protocol =
            std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
        let mut context = BcContext::new(encryption_protocol);

        let e = Bc::deserialize(&mut context, &sample[..]);
        assert_matches!(
            e,
            Ok(Bc {
                meta:
                    BcMeta {
                        msg_id: 3,
                        channel_id: 138,
                        stream_type: 0,
                        response_code: 0,
                        msg_num: 0,
                        class: 0x6414,
                    },
                body:
                    BcBody::ModernMsg(ModernMsg {
                        extension: None,
                        payload:
                            Some(BcPayloads::BcXml(BcXml {
                                preview:
                                    Some(Preview {
                                        version,
                                        channel_id: 0,
                                        handle: 0,
                                        stream_type,
                                    }),
                                ..
                            })),
                    }),
            }) if version == "1.1" && stream_type == Some("mainStream".to_string())
        );
    }
}

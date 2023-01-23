use super::model::*;
use super::xml::BcPayloads;
use super::xml_crypto;
use cookie_factory::bytes::*;
use cookie_factory::sequence::tuple;
use cookie_factory::{combinator::*, gen};
use cookie_factory::{GenError, SerializeFn, WriteContext};
use err_derive::Error;
use log::error;
use std::io::Write;

/// The error types used during serialisation
#[derive(Debug, Error, Clone)]
pub enum Error {
    /// A Cookie Factor  GenError
    #[error(display = "Cookie GenError")]
    GenError(#[error(source)] std::sync::Arc<GenError>),
}

impl From<GenError> for Error {
    fn from(k: GenError) -> Self {
        Error::GenError(std::sync::Arc::new(k))
    }
}

impl Bc {
    pub(crate) fn serialize<W: Write>(
        &self,
        buf: W,
        encryption_protocol: &EncryptionProtocol,
    ) -> Result<W, GenError> {
        // Ideally this would be a combinator, but that would be hairy because we have to
        // serialize the XML to have the metadata to build the header
        let body_buf;
        let payload_offset;

        match &self.body {
            BcBody::ModernMsg(ref modern) => {
                // First serialize ext
                let (temp_buf, ext_len) = gen(
                    opt_ref(&modern.extension, |ext| {
                        bc_ext(self.meta.channel_id as u32, ext, encryption_protocol)
                    }),
                    vec![],
                )?;

                // Now get the offset of the payload
                payload_offset = if has_payload_offset(self.meta.class) {
                    // If we're required to put binary length, put 0 if we have no binary
                    Some(if modern.extension.is_some() {
                        ext_len as u32
                    } else {
                        0
                    })
                } else {
                    None
                };

                // Now get the payload part of the body and add to ext_buf
                let (temp_buf, _) = gen(
                    opt_ref(&modern.payload, |payload_offset| {
                        bc_payload(
                            self.meta.channel_id as u32,
                            payload_offset,
                            encryption_protocol,
                        )
                    }),
                    temp_buf,
                )?;
                body_buf = temp_buf;
            }

            BcBody::LegacyMsg(ref legacy) => {
                let (buf, _) = gen(bc_legacy(legacy), vec![]).map_err(|e| {
                    error!("Send error: {}", e);
                    e
                })?;
                body_buf = buf;
                payload_offset = None;
            }
        };

        // Now have enough info to create the header
        let header = BcHeader::from_meta(&self.meta, body_buf.len() as u32, payload_offset);

        let (buf, _n) = gen(tuple((bc_header(&header), slice(body_buf))), buf)?;

        Ok(buf)
    }
}

fn bc_ext<W: Write>(
    enc_offset: u32,
    xml: &Extension,
    encryption_protocol: &EncryptionProtocol,
) -> impl SerializeFn<W> {
    let xml_bytes = xml.serialize(vec![]).unwrap();
    let enc_bytes = xml_crypto::encrypt(enc_offset, &xml_bytes, encryption_protocol);
    slice(enc_bytes)
}

fn bc_payload<W: Write>(
    enc_offset: u32,
    payload: &BcPayloads,
    encryption_protocol: &EncryptionProtocol,
) -> impl SerializeFn<W> {
    let payload_bytes = match payload {
        BcPayloads::BcXml(x) => {
            let xml_bytes = x.serialize(vec![]).unwrap();
            xml_crypto::encrypt(enc_offset, &xml_bytes, encryption_protocol)
        }
        BcPayloads::Binary(x) => x.to_owned(),
    };
    slice(payload_bytes)
}

fn bc_header<W: Write>(header: &BcHeader) -> impl SerializeFn<W> {
    tuple((
        le_u32(MAGIC_HEADER),
        le_u32(header.msg_id),
        le_u32(header.body_len),
        le_u8(header.channel_id),
        le_u8(header.stream_type),
        le_u16(header.msg_num),
        le_u16(header.response_code),
        le_u16(header.class),
        opt(header.payload_offset, le_u32),
    ))
}

fn bc_legacy<W: Write>(legacy: &'_ LegacyMsg) -> impl SerializeFn<W> + '_ {
    move |out: WriteContext<W>| {
        use LegacyMsg::*;
        match legacy {
            LoginMsg { username, password } => {
                if username.len() != 32 || password.len() != 32 {
                    // Error handling could be improved here...
                    return Err(GenError::CustomError(0));
                }
                tuple((
                    slice(username),
                    slice(password),
                    // Login messages are 1836 bytes total, username/password
                    // take up 32 chars each, 1772 zeros follow
                    slice(&[0u8; 1772][..]),
                ))(out)
            }
            UnknownMsg => {
                panic!("Cannot serialize an unknown message!");
            }
        }
    }
}

/// Applies the supplied serializer with the Option's interior data if present
fn opt<W, T, F>(opt: Option<T>, ser: impl Fn(T) -> F) -> impl SerializeFn<W>
where
    F: SerializeFn<W>,
    T: Copy,
    W: Write,
{
    move |buf: WriteContext<W>| {
        if let Some(val) = opt {
            ser(val)(buf)
        } else {
            do_nothing()(buf)
        }
    }
}

fn opt_ref<'a, W, T, F, S>(opt: &'a Option<T>, ser: S) -> impl SerializeFn<W> + 'a
where
    F: SerializeFn<W>,
    W: Write,
    S: Fn(&'a T) -> F + 'a,
{
    move |buf: WriteContext<W>| {
        if let Some(ref val) = opt {
            ser(val)(buf)
        } else {
            do_nothing()(buf)
        }
    }
}

/// A serializer combinator that does nothing with its input
fn do_nothing<W>() -> impl SerializeFn<W> {
    Ok
}

#[test]
fn test_legacy_login_roundtrip() {
    let encryption_protocol =
        std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
    let mut context = BcContext::new(encryption_protocol);

    // I don't want to make up a sample message; just load it
    let sample = include_bytes!("samples/model_sample_legacy_login.bin");
    let msg = Bc::deserialize::<&[u8]>(&mut context, &sample[..]).unwrap();

    let ser_buf = msg
        .serialize(vec![], &EncryptionProtocol::BCEncrypt)
        .unwrap();
    let msg2 = Bc::deserialize::<&[u8]>(&mut context, ser_buf.as_ref()).unwrap();
    assert_eq!(msg, msg2);
    assert_eq!(&sample[..], ser_buf.as_slice());
}

#[test]
fn test_modern_login_roundtrip() {
    let encryption_protocol =
        std::sync::Arc::new(std::sync::Mutex::new(EncryptionProtocol::BCEncrypt));
    let mut context = BcContext::new(encryption_protocol);

    // I don't want to make up a sample message; just load it
    let sample = include_bytes!("samples/model_sample_modern_login.bin");

    let msg = Bc::deserialize::<&[u8]>(&mut context, &sample[..]).unwrap();

    let ser_buf = msg
        .serialize(vec![], &EncryptionProtocol::BCEncrypt)
        .unwrap();
    let msg2 = Bc::deserialize::<&[u8]>(&mut context, ser_buf.as_ref()).unwrap();
    assert_eq!(msg, msg2);
}

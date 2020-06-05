use super::model::*;
use super::xml::BcXml;
use super::xml_crypto;
use cookie_factory::bytes::*;
use cookie_factory::sequence::tuple;
use cookie_factory::{combinator::*, gen};
use cookie_factory::{GenError, SerializeFn, WriteContext};
use std::io::Write;

pub type Error = GenError;

impl Bc {
    pub fn serialize<W: Write>(&self, buf: W) -> Result<W, GenError> {
        // Ideally this would be a combinator, but that would be hairy because we have to
        // serialize the XML to have the metadata to build the header
        let body_buf;
        let bin_offset;
        match &self.body {
            BcBody::ModernMsg(ref modern) => {
                let (buf, xml_len) = gen(
                    opt_ref(&modern.xml, |xml| bc_xml(self.meta.client_idx, xml)),
                    vec![],
                )?;
                body_buf = buf;
                bin_offset = if has_bin_offset(self.meta.class) {
                    // If we're required to put binary length, put 0 if we have no binary
                    Some(if modern.binary.is_some() {
                        xml_len as u32
                    } else {
                        0
                    })
                } else {
                    None
                };
            }
            BcBody::LegacyMsg(ref legacy) => {
                let (buf, _) = gen(bc_legacy(legacy), vec![])?;
                body_buf = buf;
                bin_offset = None;
            }
        }

        // Now have enough info to create the header
        let header = BcHeader::from_meta(&self.meta, body_buf.len() as u32, bin_offset);

        let (mut buf, _n) = gen(tuple((bc_header(&header), slice(body_buf))), buf)?;

        // Put the binary part of the body, TODO this is poorly written
        if let BcBody::ModernMsg(ModernMsg {
            binary: Some(ref binary),
            ..
        }) = self.body
        {
            let (buf2, _) = gen(slice(binary), buf)?;
            buf = buf2
        }

        Ok(buf)
    }
}

fn bc_xml<W: Write>(enc_offset: u32, xml: &BcXml) -> impl SerializeFn<W> {
    let xml_bytes = xml.serialize(vec![]).unwrap();
    let enc_bytes = xml_crypto::crypt(enc_offset, &xml_bytes);
    slice(enc_bytes)
}

fn bc_header<W: Write>(header: &BcHeader) -> impl SerializeFn<W> {
    // TODO this is actually a u16 "response code" in modern messages
    let (signaled_encryption, spare) = if header.class == 0x0000 {
        (0x00, 0x00)
    } else {
        (header.encrypted as u8, 0xdc)
    };
    tuple((
        le_u32(MAGIC_HEADER),
        le_u32(header.msg_id),
        le_u32(header.body_len),
        le_u32(header.enc_offset),
        le_u8(signaled_encryption),
        le_u8(spare), // skipped byte
        le_u16(header.class),
        opt(header.bin_offset, le_u32),
    ))
}

fn bc_legacy<'a, W: Write>(legacy: &'a LegacyMsg) -> impl SerializeFn<W> + 'a {
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
            ser(&*val)(buf)
        } else {
            do_nothing()(buf)
        }
    }
}

/// A serializer combinator that does nothing with its input
fn do_nothing<W>() -> impl SerializeFn<W> {
    move |out: WriteContext<W>| Ok(out)
}

#[test]
fn test_legacy_login_roundtrip() {
    let mut context = BcContext::new();

    // I don't want to make up a sample message; just load it
    let sample = include_bytes!("samples/model_sample_legacy_login.bin");
    let msg = Bc::deserialize::<&[u8]>(&mut context, &sample[..]).unwrap();

    let ser_buf = msg.serialize(vec![]).unwrap();
    let msg2 = Bc::deserialize::<&[u8]>(&mut context, ser_buf.as_ref()).unwrap();
    assert_eq!(msg, msg2);
    assert_eq!(&sample[..], ser_buf.as_slice());
}

#[test]
fn test_modern_login_roundtrip() {
    let mut context = BcContext::new();

    // I don't want to make up a sample message; just load it
    let sample = include_bytes!("samples/model_sample_modern_login.bin");

    let msg = Bc::deserialize::<&[u8]>(&mut context, &sample[..]).unwrap();

    let ser_buf = msg.serialize(vec![]).unwrap();
    let msg2 = Bc::deserialize::<&[u8]>(&mut context, ser_buf.as_ref()).unwrap();
    assert_eq!(msg, msg2);
}

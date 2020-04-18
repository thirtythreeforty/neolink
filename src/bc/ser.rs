use cookie_factory::bytes::*;
use cookie_factory::{gen, combinator::*};
use cookie_factory::sequence::tuple;
use cookie_factory::{GenError, SerializeFn, WriteContext};
use std::io::Write;
use super::model::*;
use super::xml::Body;
use super::xml_crypto;

impl Bc {
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), GenError> {
        gen(bc_msg(self), buf).map(|_| ())
    }
}

fn bc_msg<'a, W: Write + 'a>(msg: &'a Bc) -> impl SerializeFn<W> + 'a {
    use BcBody::*;
    tuple((
        bc_header(&msg.header),
        match msg.body {
            ModernMsg(ref body) => bc_modern(body),
            LegacyMsg(..) => unimplemented!()
        }
    ))
}

fn bc_modern<'a, W: 'a + Write>(body: &'a ModernMsg) -> impl SerializeFn<W> + 'a {
    tuple((
        opt_ref(&body.xml, |xml| bc_xml(true, 0, xml)),
        opt_ref(&body.binary, slice),
    ))
}

fn bc_xml<W: Write>(encrypted: bool, enc_offset: u32, xml: &Body) -> impl SerializeFn<W>
{
    let xml_bytes = xml.serialize(vec!()).unwrap();
    if encrypted {
        let enc_bytes = xml_crypto::crypt(enc_offset, &xml_bytes);
        slice(enc_bytes)
    } else {
        slice(xml_bytes)
    }
}

fn bc_header<W: Write>(header: &BcHeader) -> impl SerializeFn<W> {
    tuple((
        le_u32(MAGIC_HEADER),
        le_u32(header.msg_id),
        le_u32(header.body_len),
        le_u32(header.enc_offset),
        le_u8(header.encrypted as u8),
        //le_u8(header.response_code),
        le_u8(0), // skipped byte
        le_u16(header.class),
        opt(header.bin_offset, le_u32),
    ))
}

/// Applies the supplied serializer with the Option's interior data if present
fn opt<W, T, F>(opt: Option<T>, ser: impl Fn(T) -> F) -> impl SerializeFn<W>
    where F: SerializeFn<W>, T: Copy, W: Write
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
    where F: SerializeFn<W>, W: Write, S: Fn(&'a T) -> F + 'a
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
fn test_login_roundtrip() {
    // I don't want to make up a sample message; just load it
    let sample = include_bytes!("samples/model_sample_modern_login.bin");

    let msg = Bc::deserialize(&sample[..]).unwrap();

    let mut ser_buf = vec![0; sample.len()];
    msg.serialize(&mut ser_buf[..]).unwrap();

    let msg2 = Bc::deserialize(ser_buf.as_ref()).unwrap();
    assert_eq!(msg, msg2);
}

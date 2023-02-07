//! Handles sending and recieving messages as packets
//!
//! BcMediaCodex is used with a `[tokio_util::codec::Framed]` to form complete packets
//!
use crate::bcmedia::model::*;
use crate::{Error, Result};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

pub struct BcMediaCodex {}

impl BcMediaCodex {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl Encoder<BcMedia> for BcMediaCodex {
    type Error = Error;

    fn encode(&mut self, item: BcMedia, dst: &mut BytesMut) -> Result<()> {
        let buf: Vec<u8> = Default::default();
        let buf = item.serialize(buf)?;
        dst.reserve(buf.len());
        dst.extend_from_slice(buf.as_slice());
        Ok(())
    }
}

impl Decoder for BcMediaCodex {
    type Item = BcMedia;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        match { BcMedia::deserialize(src) } {
            Ok(bc) => Ok(Some(bc)),
            Err(Error::NomIncomplete(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

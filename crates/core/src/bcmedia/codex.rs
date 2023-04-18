//! Handles sending and recieving messages as packets
//!
//! BcMediaCodex is used with a `[tokio_util::codec::Framed]` to form complete packets
//!
use crate::bcmedia::model::*;
use crate::{Error, Result};
use bytes::BytesMut;
use log::*;
use tokio_util::codec::{Decoder, Encoder};

pub struct BcMediaCodex {
    /// If true we will not search for the start of the next packet
    /// in the event that the stream appears to be corrupted
    strict: bool,
}

impl BcMediaCodex {
    pub(crate) fn new(strict: bool) -> Self {
        Self { strict }
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

    /// Since frames can cross EOF boundaries we overload this so it dosen't error if
    /// there are bytes left on the stream
    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        match self.decode(buf)? {
            Some(frame) => Ok(Some(frame)),
            None => Ok(None),
        }
    }

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        let mut amount_skipped: usize = 0;
        loop {
            match { BcMedia::deserialize(src) } {
                Ok(bc) => {
                    if amount_skipped > 0 {
                        debug!("Amount skipped to restore stream: {}", amount_skipped);
                    }
                    return Ok(Some(bc));
                }
                Err(Error::NomIncomplete(_)) => {
                    if amount_skipped > 0 {
                        debug!("Amount skipped to restore stream: {}", amount_skipped);
                    }
                    return Ok(None);
                }
                Err(e) => {
                    if self.strict {
                        return Err(e);
                    } else if src.is_empty() {
                        return Ok(None);
                    } else {
                        if amount_skipped == 0 {
                            debug!("Error in stream attempting to restore");
                            trace!("   Stream Error: {:?}", e);
                        }
                        // Drop the whole packet and wait for a packet that starts with magic
                        amount_skipped += src.len();
                        src.clear();
                        continue;
                    }
                }
            }
        }
    }
}

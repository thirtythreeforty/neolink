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
    amount_skipped: usize,
}

impl BcMediaCodex {
    pub(crate) fn new(strict: bool) -> Self {
        Self {
            strict,
            amount_skipped: 0,
        }
    }
}

impl Encoder<BcMedia> for BcMediaCodex {
    type Error = Error;

    fn encode(&mut self, item: BcMedia, dst: &mut BytesMut) -> Result<()> {
        let buf: Vec<u8> = Default::default();
        let buf = item.serialize(buf)?;
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
        loop {
            match { BcMedia::deserialize(src) } {
                Ok(bc) => {
                    if self.amount_skipped > 0 {
                        debug!("Amount skipped to restore stream: {}", self.amount_skipped);
                        self.amount_skipped = 0;
                    }
                    return Ok(Some(bc));
                }
                Err(Error::NomIncomplete(_)) => {
                    if self.amount_skipped > 0 {
                        debug!("Amount skipped to restore stream: {}", self.amount_skipped);
                        self.amount_skipped = 0;
                    }
                    return Ok(None);
                }
                Err(e) => {
                    if self.strict {
                        return Err(e);
                    } else if src.is_empty() {
                        return Ok(None);
                    } else {
                        if self.amount_skipped == 0 {
                            debug!("Error in stream attempting to restore");
                            trace!("   Stream Error: {:?}", e);
                        }
                        // Drop the whole packet and wait for a packet that starts with magic
                        self.amount_skipped += src.len();
                        src.clear();
                        continue;
                    }
                }
            }
        }
    }
}

//! Handles sending and recieving messages as complete packets
//!
//! BcUdpCodex is used with a `[tokio_util::codec::Framed]` to form complete packets
//!
use crate::bcudp::model::*;
use crate::{Error, Result};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use super::xml::UdpXml;

pub(crate) struct BcUdpCodex {}

impl BcUdpCodex {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl Encoder<BcUdp> for BcUdpCodex {
    type Error = Error;

    fn encode(&mut self, item: BcUdp, dst: &mut BytesMut) -> Result<()> {
        log::trace!("Encoding: {item:?}");
        let buf: Vec<u8> = Default::default();
        let buf = item.serialize(buf)?;
        dst.extend_from_slice(buf.as_slice());
        log::trace!("  Encoding: Done: {}", buf.len());
        Ok(())
    }
}

impl Decoder for BcUdpCodex {
    type Item = BcUdp;
    type Error = Error;

    /// Since frames can cross EOF boundaries we overload this so it dosen't error if
    /// there are bytes left on the stream
    // fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
    //     match self.decode(buf)? {
    //         Some(frame) => Ok(Some(frame)),
    //         None => Ok(None),
    //     }
    // }

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        log::trace!("Decoding:");
        if src.is_empty() {
            return Ok(None);
        }
        match { BcUdp::deserialize(src) } {
            Ok(BcUdp::Discovery(UdpDiscovery {
                payload: UdpXml {
                    r2c_disc: Some(_), ..
                },
                ..
            })) => {
                log::trace!("   Decoding: Relay terminate");
                Err(Error::RelayTerminate)
            }
            Ok(BcUdp::Discovery(UdpDiscovery {
                payload: UdpXml {
                    d2c_disc: Some(_), ..
                },
                ..
            })) => {
                log::trace!("   Decoding:Camera terminate");
                Err(Error::CameraTerminate)
            }
            Ok(bc) => {
                log::trace!("   Decoding: Ok");
                Ok(Some(bc))
            }
            Err(Error::NomIncomplete(_)) => {
                log::trace!("   Decoding: Incomplete: {:0X?}", src);
                Ok(None)
            }
            Err(e) => {
                log::trace!("   Decoding: Err");
                Err(e)
            }
        }
    }
}

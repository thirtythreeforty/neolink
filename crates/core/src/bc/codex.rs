//! Create a tokio encoder/decoder for turning a AsyncRead/Write stream into
//! a Bc packet
//!
//! BcCodex is used with a `[tokio_util::codec::Framed]` to form complete packets
//!
use crate::bc::model::*;
use crate::{Error, Result};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

pub(crate) struct BcCodex {
    context: BcContext,
}

impl BcCodex {
    pub(crate) fn new() -> Self {
        Self {
            context: BcContext::new(),
        }
    }
    pub(crate) fn get_encrypted(&self) -> &EncryptionProtocol {
        self.context.get_encrypted()
    }
    pub(crate) fn set_encrypted(&mut self, protocol: EncryptionProtocol) {
        self.context.set_encrypted(protocol);
    }
}

impl Encoder<Bc> for BcCodex {
    type Error = Error;

    fn encode(&mut self, item: Bc, dst: &mut BytesMut) -> Result<()> {
        // let context = self.context.read().unwrap();

        let buf: Vec<u8> = Default::default();
        let buf = item.serialize(buf, self.context.get_encrypted())?;
        dst.reserve(buf.len());
        dst.extend_from_slice(buf.as_slice());
        Ok(())
    }
}

impl Decoder for BcCodex {
    type Item = Bc;
    type Error = Error;
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        let bc = match { Bc::deserialize(&self.context, src) } {
            Ok(bc) => bc,
            Err(Error::NomIncomplete(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        // Update context
        if bc.meta.msg_id == 1 && (bc.meta.response_code >> 8) == 0xdd {
            // Login reply has the encryption info
            // Set that the encryption type now
            let encryption_protocol_byte = (bc.meta.response_code & 0xff) as usize;
            match encryption_protocol_byte {
                0x00 => self.context.set_encrypted(EncryptionProtocol::Unencrypted),
                0x01 => self.context.set_encrypted(EncryptionProtocol::BCEncrypt),
                0x02 => self.context.set_encrypted(EncryptionProtocol::Aes(None)),
                _ => {
                    return Err(Error::UnknownEncryption(encryption_protocol_byte));
                }
            }
        }

        if let BcBody::ModernMsg(ModernMsg {
            extension:
                Some(Extension {
                    binary_data: Some(1),
                    ..
                }),
            ..
        }) = bc.body
        {
            self.context.binary_on(bc.meta.msg_num);
        }

        Ok(Some(bc))
    }
}

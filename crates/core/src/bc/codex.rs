//! Create a tokio encoder/decoder for turning a AsyncRead/Write stream into
//! a Bc packet
//!
//! BcCodex is used with a `[tokio_util::codec::Framed]` to form complete packets
//!
use crate::bc::model::*;
use crate::bc::xml::*;
use crate::{Credentials, Error, Result};
use bytes::BytesMut;
use nom::AsBytes;
use tokio_util::codec::{Decoder, Encoder};

pub(crate) struct BcCodex {
    context: BcContext,
}

impl BcCodex {
    pub(crate) fn new_with_debug(credentials: Credentials) -> Self {
        let mut context = BcContext::new(credentials);

        context.debug_on();
        Self { context }
    }
    pub(crate) fn new(credentials: Credentials) -> Self {
        Self {
            context: BcContext::new(credentials),
        }
    }
}

impl Encoder<Bc> for BcCodex {
    type Error = Error;

    fn encode(&mut self, item: Bc, dst: &mut BytesMut) -> Result<()> {
        // let context = self.context.read().unwrap();
        let buf: Vec<u8> = Default::default();
        let enc_protocol: EncryptionProtocol = match self.context.get_encrypted() {
            EncryptionProtocol::Aes(_) | EncryptionProtocol::FullAes(_)
                if item.meta.msg_id == 1 =>
            {
                // During login the encyption protocol cannot go higher than BCEncrypt
                // even if we support AES. (BUt it can go lower i.e. None)
                EncryptionProtocol::BCEncrypt
            }
            n => *n,
        };
        let buf = item.serialize(buf, &enc_protocol)?;
        dst.extend_from_slice(buf.as_slice());
        Ok(())
    }
}

impl Decoder for BcCodex {
    type Item = Bc;
    type Error = Error;

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        match self.decode(buf)? {
            Some(frame) => Ok(Some(frame)),
            None => {
                if buf.is_empty() {
                    Ok(None)
                } else {
                    log::debug!(
                        "bytes remaining on BC stream: {:X?}",
                        buf.as_bytes().chunks(25).next()
                    );
                    // Right after this we seem to get an issue with the camera dropping us
                    // Needs probing
                    // F0, DE, BC, A, 3, 0, 0, 0, 88, 6, 0, 0, 0, 1, 4, 0, C8, 0, 0, 0, 0, 0, 0, 0, 30, 31, 64, 63, 48,
                    // 32, 36, 34, 6A, 6, 0, 0, 0, 0, 0, 0, D8, F5, C7, 86, 56, 0, 0, 0, 0, 0, 0, 1, 21, 9A, FC, 22, 7F, 6, AE, F6, 15, FF, E5, 71, 4, 2F, 24, 61, 15, 96, F0, BF, 83, DE, 10, BE, B4, 2E, 3
                    // 9, 76, 56, 92, 7E, 48, 79, 20, 9A, DC, 1B, BB, AC, 22, 60, 5C, 72, B5, 3D, 8, E0, 34, 43, 3F, 2E, A7, 81, A8, 11, 75, 7F, 58, 3E, 8, 54, 91, 43, 21, EC, 6B, D6, 1A, D5, CB, D5, 6C,
                    // 8C, 2E, 6E, A3, 51, C3, A4, F0, CF, 2B, 61, 81, D0, 1C, A1, 76, EE, BF, 7A, D5, D8, D1, C4, D, B0, 45, EE, 3E, 93, 9A, CE, 5F, AB, 75, 55, AC, 9D, 66, DE, 23, 6D, 5F, 25, 57, DA, F5
                    //, E, 7F, 8D, 30, A7, 66, C4, 60, 76, 41, D0, 6A, 23, E, A9, C5, 51, EE, F6, DD, 19, E7, A8, 96, 9F, 2B, AF, 31, 90, 9D, FC, BE
                    Ok(None)
                }
            }
        }
        // match self.decode(buf)? {
        //     Some(frame) => Ok(Some(frame)),
        //     None => Ok(None),
        // }
    }

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        // trace!("Decoding: {:X?}", src);
        let bc = Bc::deserialize(&self.context, src);
        // trace!("As: {:?}", bc);
        let bc = match bc {
            Ok(bc) => bc,
            Err(Error::NomIncomplete(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        // Update context
        if let Bc {
            meta:
                BcMeta {
                    msg_id: 1,
                    response_code,
                    ..
                },
            body:
                BcBody::ModernMsg(ModernMsg {
                    payload:
                        Some(BcPayloads::BcXml(BcXml {
                            encryption: Some(Encryption { nonce, .. }),
                            ..
                        })),
                    ..
                }),
        } = &bc
        {
            if response_code >> 8 == 0xdd {
                // Login reply has the encryption info
                // Set that the encryption type now
                let encryption_protocol_byte = (response_code & 0xff) as usize;
                match encryption_protocol_byte {
                    0x00 => self.context.set_encrypted(EncryptionProtocol::Unencrypted),
                    0x01 => self.context.set_encrypted(EncryptionProtocol::BCEncrypt),
                    0x02 => self.context.set_encrypted(EncryptionProtocol::Aes(
                        self.context.credentials.make_aeskey(nonce),
                    )),
                    0x12 => self.context.set_encrypted(EncryptionProtocol::FullAes(
                        self.context.credentials.make_aeskey(nonce),
                    )),
                    _ => {
                        return Err(Error::UnknownEncryption(encryption_protocol_byte));
                    }
                }
            }
        }

        if let BcBody::ModernMsg(ModernMsg {
            extension:
                Some(Extension {
                    binary_data: Some(on_off),
                    ..
                }),
            ..
        }) = bc.body
        {
            if on_off == 0 {
                self.context.binary_off(bc.meta.msg_num);
            } else {
                self.context.binary_on(bc.meta.msg_num);
            }
        }

        Ok(Some(bc))
    }
}

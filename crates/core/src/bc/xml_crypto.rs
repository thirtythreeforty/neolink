use super::model::EncryptionProtocol;
use aes::Aes128;
use cfb_mode::cipher::{NewStreamCipher, StreamCipher};
use cfb_mode::Cfb;

const XML_KEY: [u8; 8] = [0x1F, 0x2D, 0x3C, 0x4B, 0x5A, 0x69, 0x78, 0xFF];
const IV: &[u8] = b"0123456789abcdef";

pub fn decrypt(offset: u32, buf: &[u8], encryption_protocol: &EncryptionProtocol) -> Vec<u8> {
    match encryption_protocol {
        EncryptionProtocol::Unencrypted => buf.to_vec(),
        EncryptionProtocol::BCEncrypt => {
            let key_iter = XML_KEY.iter().cycle().skip(offset as usize % 8);
            key_iter
                .zip(buf)
                .map(|(key, i)| *i ^ key ^ (offset as u8))
                .collect()
        }
        EncryptionProtocol::Aes(aeskey) => {
            // AES decryption

            let mut decrypted = buf.to_vec();
            Cfb::<Aes128>::new(aeskey.into(), IV.into()).decrypt(&mut decrypted);
            decrypted
        }
    }
}

pub fn encrypt(offset: u32, buf: &[u8], encryption_protocol: &EncryptionProtocol) -> Vec<u8> {
    match encryption_protocol {
        EncryptionProtocol::Unencrypted => {
            // Encrypt is the same as decrypt
            decrypt(offset, buf, encryption_protocol)
        }
        EncryptionProtocol::BCEncrypt => {
            // Encrypt is the same as decrypt
            decrypt(offset, buf, encryption_protocol)
        }
        EncryptionProtocol::Aes(aeskey) => {
            // AES encryption
            let mut encrypted = buf.to_vec();
            Cfb::<Aes128>::new(aeskey.into(), IV.into()).encrypt(&mut encrypted);
            encrypted
        }
    }
}

#[test]
fn test_xml_crypto() {
    let sample = include_bytes!("samples/xml_crypto_sample1.bin");
    let should_be = include_bytes!("samples/xml_crypto_sample1_plaintext.bin");

    let decrypted = decrypt(0, &sample[..], &EncryptionProtocol::BCEncrypt);
    assert_eq!(decrypted, &should_be[..]);
}

#[test]
fn test_xml_crypto_roundtrip() {
    let zeros: [u8; 256] = [0; 256];

    let decrypted = encrypt(0, &zeros[..], &EncryptionProtocol::BCEncrypt);
    let encrypted = decrypt(0, &decrypted[..], &EncryptionProtocol::BCEncrypt);
    assert_eq!(encrypted, &zeros[..]);
}

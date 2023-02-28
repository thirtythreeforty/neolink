//! Use to decode AES packet data
use hex_string::HexString;
use requestty::Question;
use std::convert::TryInto;

use aes::Aes128;
use cfb_mode::cipher::{NewStreamCipher, StreamCipher};
use cfb_mode::Cfb;

const IV: &[u8] = b"0123456789abcdef";

fn decrypt(buf: &[u8], aeskey: &[u8; 16]) -> Vec<u8> {
    let mut decrypted = buf.to_vec();
    Cfb::<Aes128>::new(aeskey.into(), IV.into()).decrypt(&mut decrypted);
    decrypted
}

fn make_aeskey<T: AsRef<str>>(password: T, nonce: T) -> [u8; 16] {
    let key_phrase = format!("{}-{}", nonce.as_ref(), password.as_ref(),);
    let key_phrase_hash = format!("{:X}\0", md5::compute(key_phrase))
        .to_uppercase()
        .into_bytes();
    key_phrase_hash[0..16].try_into().unwrap()
}

fn main() -> Result<(), anyhow::Error> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let question = Question::input("hex")
        .message("Enter the hex string to decode")
        .build();

    let source_str: String = requestty::prompt_one(question)?
        .as_string()
        .ok_or_else(|| anyhow::anyhow!("Did not get reply"))?
        .to_string();

    let question = Question::password("password")
        .message("Enter Camera Password")
        .mask('*')
        .build();
    let pass = requestty::prompt_one(question)?
        .as_string()
        .ok_or_else(|| anyhow::anyhow!("Did not get reply"))?
        .to_string();

    let question = Question::password("nonce")
        .message("Enter Login Nonce")
        .mask('*')
        .build();
    let nonce = requestty::prompt_one(question)?
        .as_string()
        .ok_or_else(|| anyhow::anyhow!("Did not get reply"))?
        .to_string();

    let source_hex = HexString::from_string(&source_str).unwrap();

    let decrypted = decrypt(&source_hex.as_bytes(), &make_aeskey(&pass, &nonce));
    log::info!("Bytes: {:X?}", decrypted);
    log::info!("Text: {}", String::from_utf8(decrypted)?);

    Ok(())
}

const XML_KEY: [u8; 8] = [0x1F, 0x2D, 0x3C, 0x4B, 0x5A, 0x69, 0x78, 0xFF];

pub fn crypt(offset: u32, buf: &[u8]) -> Vec<u8> {
    let key_iter = XML_KEY.iter().cycle().skip(offset as usize % 8);
    key_iter
        .zip(buf)
        .map(|(key, i)| *i ^ key ^ (offset as u8))
        .collect()
}

#[test]
fn test_xml_crypto() {
    let sample = include_bytes!("samples/xml_crypto_sample1.bin");
    let should_be = include_bytes!("samples/xml_crypto_sample1_plaintext.bin");

    let decrypted = crypt(0, &sample[..]);
    assert_eq!(decrypted, &should_be[..]);
}

#[test]
fn test_xml_crypto_roundtrip() {
    let zeros: [u8; 256] = [0; 256];

    let decrypted = crypt(0, &zeros[..]);
    let encrypted = crypt(0, &decrypted[..]);
    assert_eq!(encrypted, &zeros[..]);
}

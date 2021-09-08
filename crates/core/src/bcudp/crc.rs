use crc32fast::Hasher;

pub(crate) fn calc_crc(payload: &[u8]) -> u32 {
    // Bc uses a non standard crc.
    //
    // It uses the polynomial 0x04c11db7
    // It uses the inital value or 0x00000000
    // It uses the xorout of 0x00000000
    //
    // The crc32fast has an odd behavior were it bitwise negates
    // the initial value before the loop. In order to have
    // an effective initial value of 0x00000000 we need to provide
    // the value 0xffffffff
    let mut hasher = Hasher::new_with_initial(0xffffffff);
    hasher.update(payload);
    // crc32fast uses the algorithm CRC-32/ISO-HDLC
    // This has an xorout of 0xffffffff
    // we must undo this xorout
    hasher.finalize() ^ 0xffffffff_u32
}

use crc32fast::Hasher;

pub(crate) fn calc_crc(payload: &[u8]) -> u32 {
    let mut hasher = Hasher::new_with_initial(0xffffffff);
    hasher.update(payload);
    hasher.finalize() ^ 0xffffffff_u32
}

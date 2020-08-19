/*
 This is a rust implementation of OKI ADPCM.
*/
use std::convert::TryInto;

struct AdpcmSetup {
  max_step_index: usize,
  steps: Vec<usize>,
  max_sample_size: i16,
  changes: Vec<isize>,
  mask: u8,
}

impl AdpcmSetup {
    fn new_oki() -> Self {
        Self{
            max_step_index: 48,
            steps: vec![16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45,
                        50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143,
                        157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
                        494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552],
            changes: vec![-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8],
            max_sample_size: 2048,
            mask: 15,
        }
    }
}

pub fn oki_to_pcm(bytes: &[u8]) -> Vec<u8> {
    let oki_context = AdpcmSetup::new_oki();
    let mut result = vec![];

    const OKI_MAGIC: &[u8] = &[0x00, 0x01, 0x7A, 0x00];

    const MAX_I16: i16 = 32767;

    let magic = &bytes[0..4];
    assert!(magic == OKI_MAGIC, "Unexpected oki magic code");
    let last_output_byes = &bytes[4..6];
    let step_index_bytes = &bytes[6..8];
    let data = &bytes[8..];

    let mut step_index: usize = u16::from_le_bytes(step_index_bytes.try_into().expect("slice with incorrect length")) as usize; // Index is inti to 16 in oki
    let mut last_output: i16 = i16::from_le_bytes(last_output_byes.try_into().expect("slice with incorrect length"));  // PCM is i16, init to 0 in oki
    let mut step: usize = oki_context.steps[step_index];
    for byte in data {
        let mask = oki_context.mask;
        // High nibble first
        let nibbles: &[u8; 2] = &[ (*byte & mask), ((*byte & mask << 4) >> 4) ];
        for nibble in nibbles {
            step_index = match step_index as isize + oki_context.changes[*nibble as usize] { // Keep it in max index range
                n if n < 0 => 0,
                n if n > oki_context.max_step_index as isize => oki_context.max_step_index,
                n => n as usize,
            };
            let sign = match (*nibble & 8) >> 3 {
                1 => -1,
                _ => 1
            };
            let magnitude = (*nibble & 7) as i8;
            let signed_nibble = magnitude * sign; // Signed nibble for the delta
            let diff = (step as isize) * (signed_nibble as isize) /2 + (step as isize) /8;
            let raw_sample = last_output as isize + diff;
            let sample = match raw_sample { // Keep it in max sample range
                sample if sample > oki_context.max_sample_size as isize => oki_context.max_sample_size,
                sample if sample < -oki_context.max_sample_size  as isize => -oki_context.max_sample_size,
                sample => sample as i16,
            };
            let scaled_sample = (sample as isize * MAX_I16 as isize / oki_context.max_sample_size as isize) as i16;

            result.extend(scaled_sample.to_le_bytes().iter());
            last_output = sample;
            step = oki_context.steps[step_index];
        }
    };

    result
}

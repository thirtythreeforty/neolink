/*
 This is a rust implementation of OKI ADPCM.
*/
use std::convert::TryInto;

struct AdpcmSetup {
  max_step_index: usize,
  steps: Vec<usize>,
  max_sample_size: isize,
  changes: Vec<isize>,
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
        }
    }
}

struct Nibble { // A nibble is a 4bit int
    data: u8, // This is the raw data for the nibble
}

impl Nibble {
    // Use u/isize throughout to ensure that we always have enough
    // Headroom to do the math without needing `as` casting everywhere
    fn unsigned(&self) -> usize {
        (self.data & 0b00001111) as usize // Mask first 4 bits it just to be sure its in nibble range
    }

    fn signed_magnitude(&self) -> usize {
        (self.data & 0b00000111) as usize  // Mask of first 3 bits which are the magnitiude bits in signed int
    }

    fn signed(&self) -> isize {
        match self.data & 0b00001000  { // Sign bit is at the 4th bit
            0b00001000 => -(self.signed_magnitude() as isize),
            _ => self.signed_magnitude() as isize,
        }
    }

    fn from_byte(byte: &u8) -> [Self; 2] { // Two nibbles per byte
        [
            Self{
                data: byte & 0b00001111,
            },
            Self{
                data: (byte & 0b11110000) >> 4,
            }
        ]
    }
}

pub fn oki_to_pcm(bytes: &[u8]) -> Vec<u8> {
    let oki_context = AdpcmSetup::new_oki(); // Should be able to get other adpcm by changing this
    let mut result = vec![]; // Stores the PCM byte array as it is built

    // ADPCM is not really a streamable format
    // Each audio sample requires information on the previous sample
    // To solve this reolink caches the intermediate variables (step_index and last_output)
    // Into a sub-header in the stream. When ever an adpcm packet arrives it starts with
    // 0x00017A00 which is the magic (I think) following by
    // 0xYY which is the last output
    // 0xZZ which is the step index of the last output
    // We must initialise our decoder with this data

    // To avoid casting to u8 <-> u16 <-> u32 and back all the time I just do all maths in u/isize
    // This gives enough headroom to do all calculations without overflow because oki puts artifical
    // limits on the sample sizes

    // Check for valid magic panic if not
    const OKI_MAGIC: &[u8] = &[0x00, 0x01, 0x7A, 0x00];
    let magic = &bytes[0..4];
    assert!(magic == OKI_MAGIC, "Unexpected oki magic code {:x?}", &magic);

    // Get the cached intermediate variables from the subheader
    let last_output_byes = &bytes[4..6];
    let step_index_bytes = &bytes[6..8];
    let mut step_index: isize = u16::from_le_bytes(step_index_bytes.try_into().expect("slice with incorrect length")) as isize; // Index is inti to 16 in oki
    let mut last_output: isize = i16::from_le_bytes(last_output_byes.try_into().expect("slice with incorrect length")) as isize;  // PCM is i16, init to 0 in oki
    let mut step: usize;

    // The rest is all data to be decoded
    let data = &bytes[8..];

    for byte in data {
        let nibbles: [Nibble; 2] = Nibble::from_byte(byte);
        for nibble in &nibbles {
            let unibble = nibble.unsigned();
            let inibble = nibble.signed();

            // Specifications say: Clamp it in max index range 0..oki_context.max_step_index
            step_index = match step_index {
                n if n < 0 => 0,
                n if n > oki_context.max_step_index as isize => oki_context.max_step_index as isize,
                n => n,
            };

            // This is just Eulers approximation with a variable step size
            // **Adaptive** Differential PCM
            // Adaptive: because the step size is variable
            step = oki_context.steps[step_index as usize];

            // Calculate the delta (which is really what adpcm is all about)
            // Adaptive **Differential** PCM
            // Differential: Becuase its all about the difference (gradient)
            let diff = (step as isize) * (inibble) /2 + (step as isize) /8;

            // Eulers approxiation
            // Sample = Previous_Sample + difference*step_size
            let raw_sample = last_output + diff;

            // Specifications say: Clamp it in max sample range -oki_context.max_sample_size..oki_context.max_sample_size
            let sample = match raw_sample {
                sample if sample > oki_context.max_sample_size => oki_context.max_sample_size,
                sample if sample < -oki_context.max_sample_size => -oki_context.max_sample_size,
                sample => sample,
            };

            // PCM is really in i16 range
            // OKI has an upper limit of I15....
            // To convert we must scale it to the I16 range
            // We also cast to I16 at this point ready for the conversion to u8 bytes of the output
            let scaled_sample = (sample as isize * (i16::MAX as isize) / oki_context.max_sample_size as isize) as i16;

            // Get the results in bytes
            result.extend(scaled_sample.to_le_bytes().iter());

            // cache the last_output ready for next run
            last_output = sample;

            // Increment the step index
            step_index = match step_index as isize + oki_context.changes[unibble] {
                n if n < 0 => 0,
                n if n > oki_context.max_step_index as isize => oki_context.max_step_index as isize,
                n => n,
            };
        }
    };

    result
}

/*
 This is a rust implementation of OKI and DVI/IMA ADPCM.
*/
use super::errors::Error;
use log::error;
use std::convert::TryInto;

struct AdpcmSetup {
    max_step_index: u32,
    steps: &'static [u32],
    max_sample_size: i32,
    changes: &'static [i32],
}

impl AdpcmSetup {
    // Unused, originally we thought BC might be using OKI but it is actually DVI4
    #[allow(dead_code)]
    fn new_oki() -> Self {
        Self {
            max_step_index: 48,
            steps: &[
                16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97,
                107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
                494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552,
            ],
            changes: &[-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8],
            max_sample_size: 2048,
        }
    }

    // This is IMA format, but it is the same as DVI4 format except in the block header
    fn new_ima() -> Self {
        Self {
            max_step_index: 88,
            steps: &[
                7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50,
                55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279,
                307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282,
                1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871,
                5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818,
                18500, 20350, 22385, 24623, 27086, 29794, 32767,
            ],
            changes: &[-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8],
            max_sample_size: 32768,
        }
    }
}

struct Nibble {
    // A nibble is a 4bit int
    data: u8, // This is the raw data for the nibble
}

impl Nibble {
    // Use u/i32 throughout to ensure that we always have enough
    // Headroom to do the math without needing `as` casting everywhere
    fn unsigned(&self) -> u32 {
        (self.data & 0b00001111) as u32 // Mask first 4 bits it just to be sure its in nibble range
    }

    #[allow(dead_code)]
    fn signed_magnitude(&self) -> u32 {
        (self.data & 0b00000111) as u32 // Mask of first 3 bits which are the magnitiude bits in signed int
    }

    #[allow(dead_code)]
    fn signed(&self) -> i32 {
        match self.data & 0b00001000 {
            // Sign bit is at the 4th bit
            0b00001000 => -(self.signed_magnitude() as i32),
            _ => self.signed_magnitude() as i32,
        }
    }

    fn from_byte(byte: &u8) -> [Self; 2] {
        // Two nibbles per byte
        [
            Self {
                data: (byte & 0b11110000) >> 4,
            },
            Self {
                data: byte & 0b00001111,
            },
        ]
    }
}

pub(crate) fn adpcm_to_pcm(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let context = AdpcmSetup::new_ima();

    let mut result: Vec<u8> = vec![]; // Stores the PCM byte array

    // ADPCM is not really a streamable format
    // Each audio sample requires information on the previous sample
    // To solve this reolink caches the intermediate variables (step_index and last_output)
    // Into the block header in the stream. When ever an adpcm packet arrives it starts with
    // 0x0001 which is the frame time from HISilicon documentation (I think) following by
    // 0xWW which is half the block size
    // 0xYY which is the last output
    // 0xZZ which is the step index of the last output
    // We must initialise our decoder with this data

    if bytes.len() < 4 {
        error!("ADPCM data is too short for even the magic.");
        return Err(Error::AdpcmDecoding(
            "ADPCM data is too short for even the magic.",
        ));
    }

    // Check for valid number of frame type
    let frame_type_bytes = &bytes[0..2];
    const FRAME_TYPE_HISILICON: &[u8] = &[0x00, 0x01];
    if frame_type_bytes != FRAME_TYPE_HISILICON {
        error!("Unexpected ADPCM frame type: {:x?}", frame_type_bytes);
        return Err(Error::AdpcmDecoding("Unexpected ADPCM frame type"));
    }

    // Check for valid block size
    let block_size_bytes = &bytes[2..4];
    let block_size = (u16::from_le_bytes(
        block_size_bytes
            .try_into()
            .expect("slice with incorrect length"),
    ) as u32)
        * 2; // Block size is stored as 1/2 (don't know why)
    let full_block_size = block_size + 4; // block_size + magic (2 bytes) + size (2 bytes)
    if !bytes.len() % full_block_size as usize == 0 {
        error!("ADPCM Data is not a multiple of the block size");
        return Err(Error::AdpcmDecoding(
            "ADPCM block size does not match data length.",
        ));
    }

    // Chunk on block size
    for bytes in bytes.chunks(full_block_size as usize) {
        // Get predictor state from block header using DVI 4 format.
        if bytes.len() < 8 {
            error!("ADPCM Block size is not long enough for header");
            return Err(Error::AdpcmDecoding("ADPCM has insufficent block size"));
        }
        let step_output_bytes = &bytes[4..6];
        let mut last_output = i16::from_le_bytes(
            step_output_bytes
                .try_into()
                .expect("slice with incorrect length"),
        ) as i32;
        let step_index_bytes = &bytes[6..8];
        let mut step_index = u16::from_le_bytes(
            step_index_bytes
                .try_into()
                .expect("slice with incorrect length"),
        ) as i32;

        // To avoid casting to u8 <-> u16 <-> u32 and back all the time I just do all maths in u/i32
        // This gives enough headroom to do all calculations without overflow because adpcm puts artifical
        // limits on the sample sizes
        let mut step: u32;

        // The rest is all data to be decoded
        let data = &bytes[8..];

        for byte in data {
            let nibbles: [Nibble; 2] = Nibble::from_byte(byte);
            for nibble in &nibbles {
                let unibble = nibble.unsigned();

                // Specifications say: Clamp it in max index range 0..context.max_step_index
                step_index = match step_index {
                    n if n < 0 => 0,
                    n if n > context.max_step_index as i32 => context.max_step_index as i32,
                    n => n,
                };

                // This is just Eulers approximation with a variable step size
                // **Adaptive** Differential PCM
                // Adaptive: because the step size is variable
                step = context.steps[step_index as usize];

                let raw_sample;
                /* == Non approxiate version ===
                // This is the full maths version
                // We don't use this one as we need to match the way the encoder
                // works if we want to use the state stored in the header.
                // I have Left it here as it is easier to understand then the bit shift version below
                let inibble = nibble.signed();

                // Calculate the delta (which is really what adpcm is all about)
                // Adaptive **Differential** PCM
                // Differential: Becuase its all about the difference (gradient)
                let diff = (step as i32) * (inibble) / 2 + (step as i32) / 8;

                // Eulers approxiation
                // Sample = Previous_Sample + difference*step_size
                raw_sample = last_output + diff;
                */

                // === Approximate version ==
                // Approximate form uses bit shift operators.
                // This is a legacy of the days when mult/divides were expensive
                // It is also the format used on low end CPUs like cameras
                let mut diff = step >> 3;
                if (unibble & 0b0100) == 0b0100 {
                    diff += step;
                }
                if (unibble & 0b0010) == 0b0010 {
                    diff += step >> 1;
                }
                if (unibble & 0b0001) == 0b0001 {
                    diff += step >> 2;
                }
                // Sign test
                if (unibble & 0b1000) == 0b1000 {
                    raw_sample = last_output - (diff as i32);
                } else {
                    raw_sample = last_output + (diff as i32);
                }

                // Specifications say: Clamp it in max sample range -context.max_sample_size..context.max_sample_size
                let sample = match raw_sample {
                    value if value > context.max_sample_size - 1 => context.max_sample_size - 1,
                    value if value < -context.max_sample_size => -context.max_sample_size,
                    value => value,
                };

                // PCM is really in i16 range
                // Some formats e.g. OKI are not in the full PCM range of values
                // To convert we must scale it to the i16 range
                // We also cast to i16 at this point ready for the conversion to u8 bytes of the output
                let scaled_sample = (sample as i32 * (std::i16::MAX as i32)
                    / (context.max_sample_size - 1) as i32)
                    as i16;

                // Get the results in bytes
                result.extend(scaled_sample.to_le_bytes().iter());

                // Increment the step index
                step_index = step_index as i32 + context.changes[unibble as usize];

                // cache the last_output ready for next run
                last_output = sample;
            }
        }
    }
    Ok(result)
}

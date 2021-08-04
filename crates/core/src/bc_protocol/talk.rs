use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::{bc::model::*, bc::xml::*, bcmedia::model::*};

impl BcCamera {
    ///
    /// Send sound to the camera
    ///
    /// long description
    ///
    /// # Parameters
    ///
    /// * `adpcm` - Data must be adpcm in DVI-4 format
    ///
    /// * `block_size` - is block-align used to encode the adpcm
    ///
    /// * `sample_rate` - sample rate of the audio
    ///
    ///
    pub fn talk(&self, adpcm: &[u8], block_size: u16, sample_rate: u16) -> Result<()> {
        let connection = self.connection.as_ref().expect("Must be connected to ping");

        let sub = connection.subscribe(MSG_ID_TALKCONFIG)?;

        let talk_config = TalkConfig {
            channel_id: self.channel_id,
            duplex: "FDX".to_string(),
            audio_stream_mode: "followVideoStream".to_string(),
            audio_config: AudioConfig {
                audio_type: "adpcm".to_string(),
                sample_rate,
                sample_precision: 16,
                length_per_encoder: block_size * 2,
                soundTrack: "mono".to_string(),
            },
            ..Default::default()
        };

        let msg = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_TALKCONFIG,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                stream_type: 0,
                response_code: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(Extension {
                    channel_id: Some(self.channel_id),
                    ..Default::default()
                }),
                payload: Some(BcPayloads::BcXml(BcXml {
                    talk_config: Some(talk_config),
                    ..Default::default()
                })),
            }),
        };

        sub.send(msg)?;
        let msg = sub.rx.recv_timeout(RX_TIMEOUT)?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
        } else {
            return Err(Error::UnintelligibleReply {
                reply: msg,
                why:
                    "The camera did not accept the TalkConfig xml. Audio format is likely incorrect",
            });
        }

        let full_block_size = block_size + 4; // Block size + predictor state
        let sub = connection.subscribe(MSG_ID_TALK)?;

        const BLOCK_PER_PAYLOAD: usize = 4;
        const BLOCK_HEADER_SIZE: usize = 4;
        const SAMPLES_PER_BYTE: usize = 2;

        for payload_bytes in adpcm.chunks(full_block_size as usize * BLOCK_PER_PAYLOAD) {
            let mut payload = vec![];
            for bytes in payload_bytes.chunks(full_block_size as usize) {
                let bcmedia_adpcm = BcMedia::Adpcm(BcMediaAdpcm {
                    data: bytes.to_vec(),
                });
                payload = bcmedia_adpcm.serialize(payload)?;
            }

            let msg = Bc {
                meta: BcMeta {
                    msg_id: MSG_ID_TALK,
                    channel_id: self.channel_id,
                    msg_num: self.new_message_num(),
                    stream_type: 0,
                    response_code: 0,
                    class: 0x6414,
                },
                body: BcBody::ModernMsg(ModernMsg {
                    extension: Some(Extension {
                        channel_id: Some(self.channel_id),
                        binary_data: Some(1),
                        ..Default::default()
                    }),
                    payload: Some(BcPayloads::Binary(payload)),
                }),
            };

            sub.send(msg)?;

            let adpcm_len = payload_bytes.len();
            // There are two samples per byte
            //
            // To calculate the bytes we subtract the block headers from the len
            //
            // There is 1 initial sample stored in the block header so we add that in the end
            //
            let samples_sent = (adpcm_len - BLOCK_HEADER_SIZE * BLOCK_PER_PAYLOAD)
                * SAMPLES_PER_BYTE
                + BLOCK_PER_PAYLOAD;

            // Time to play the sample in seconds
            let play_length = samples_sent as f32 / sample_rate as f32;
            std::thread::sleep(std::time::Duration::from_secs_f32(play_length));
        }

        Ok(())
    }
}

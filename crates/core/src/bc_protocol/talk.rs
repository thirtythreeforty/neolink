use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::{bc::model::*, bc::xml::*, bcmedia::model::*};
use crossbeam_channel::Receiver;
use std::io::{BufRead, Error as IoError, ErrorKind, Read};

type IoResult<T> = std::result::Result<T, IoError>;

impl BcCamera {
    ///
    /// Finish Talk
    ///
    /// The send the talk finish to the camera.
    ///
    /// It is also sent when the request for talk config returns status code 422
    ///
    pub fn talk_stop(&self) -> Result<()> {
        let connection = self.get_connection();

        let msg_num = self.new_message_num();
        let sub = connection.subscribe(msg_num)?;

        let msg = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_TALKRESET,
                channel_id: self.channel_id,
                msg_num,
                stream_type: 0,
                response_code: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(Extension {
                    channel_id: Some(self.channel_id),
                    ..Default::default()
                }),
                payload: None,
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
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the talk stop command.",
            });
        }

        Ok(())
    }

    ///
    /// Requests the [`TalkAbility`] xml
    ///
    pub fn talk_ability(&self) -> Result<TalkAbility> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let sub_get = connection.subscribe(msg_num)?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_TALKABILITY,
                channel_id: self.channel_id,
                msg_num,
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(Extension {
                    channel_id: Some(self.channel_id),
                    ..Default::default()
                }),
                payload: None,
            }),
        };

        sub_get.send(get)?;
        let msg = sub_get.rx.recv_timeout(RX_TIMEOUT)?;

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    talk_ability: Some(talk_ability),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(talk_ability)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected TalkAbility xml but it was not recieved",
            })
        }
    }

    ///
    /// Send sound to the camera
    ///
    /// The data should be in the format as described in `<TalkAbility>` xml
    /// This method assumes that you have set up the data in the desired format
    /// in the `<TalkAbility>` xml
    ///
    /// It also checks that it is ADPCM as the code is written to accept only that
    ///
    /// # Parameters
    ///
    /// * `adpcm` - Data must be adpcm in DVI-4 format
    ///
    /// * `talk_config` - The talk config that describes the adpcm data
    ///
    ///
    pub fn talk(&self, adpcm: &[u8], talk_config: TalkConfig) -> Result<()> {
        let connection = self.get_connection();

        let msg_num = self.new_message_num();
        let sub = connection.subscribe(msg_num)?;

        if &talk_config.audio_config.audio_type != "adpcm" {
            return Err(Error::UnknownTalkEncoding);
        }

        let block_size = talk_config.audio_config.length_per_encoder / 2;
        let sample_rate = talk_config.audio_config.sample_rate;

        let msg = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_TALKCONFIG,
                channel_id: self.channel_id,
                msg_num,
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
        let mut msg = sub.rx.recv_timeout(RX_TIMEOUT)?;

        // If another client is already talking OR if we crashed before sending
        // msgid 11 the camera will reply 422
        //
        // The official client in this case just sends msgid 11 and retries so
        // we will do that too
        if let BcMeta {
            response_code: 422, ..
        } = msg.meta
        {
            self.talk_stop()?;
            // Retry
            sub.send(msg)?;
            msg = sub.rx.recv_timeout(RX_TIMEOUT)?;
        }

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
        } else {
            return Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why:
                    "The camera did not accept the TalkConfig xml. Audio format is likely incorrect",
            });
        }

        let full_block_size = block_size + 4; // Block size + predictor state
        let msg_num = self.new_message_num();
        let sub = connection.subscribe(msg_num)?;

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
                    msg_num,
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

        self.talk_stop()?;

        Ok(())
    }

    ///
    /// Send sound to the camera through a channel
    ///
    /// This is similar to [`talk`] except it uses a channel to receive data
    ///
    /// The data should be in the format as described in `<TalkAbility>` xml
    /// This method assumes that you have set up the data in the desired format
    /// in the `<TalkAbility>` xml
    ///
    /// It also checks that it is ADPCM as the code is written to accept only that
    ///
    /// # Parameters
    ///
    /// * `adpcm` - Data must be adpcm in DVI-4 format
    ///
    /// * `talk_config` - The talk config that describes the adpcm data
    ///
    ///
    pub fn talk_stream(&self, rx: Receiver<Vec<u8>>, talk_config: TalkConfig) -> Result<()> {
        let connection = self.get_connection();

        let msg_num = self.new_message_num();
        let sub = connection.subscribe(msg_num)?;

        if &talk_config.audio_config.audio_type != "adpcm" {
            return Err(Error::UnknownTalkEncoding);
        }

        let block_size = talk_config.audio_config.length_per_encoder / 2;
        let sample_rate = talk_config.audio_config.sample_rate;

        let msg = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_TALKCONFIG,
                channel_id: self.channel_id,
                msg_num,
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
        let mut msg = sub.rx.recv_timeout(RX_TIMEOUT)?;

        // If another client is already talking OR if we crashed before sending
        // msgid 11 the camera will reply 422
        //
        // The official client in this case just sends msgid 11 and retries so
        // we will do that too
        if let BcMeta {
            response_code: 422, ..
        } = msg.meta
        {
            self.talk_stop()?;
            // Retry
            sub.send(msg)?;
            msg = sub.rx.recv_timeout(RX_TIMEOUT)?;
        }

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
        } else {
            return Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why:
                    "The camera did not accept the TalkConfig xml. Audio format is likely incorrect",
            });
        }

        let full_block_size = block_size + 4; // Block size + predictor state
        let msg_num = self.new_message_num();
        let sub = connection.subscribe(msg_num)?;

        const BLOCK_PER_PAYLOAD: usize = 1;
        const BLOCK_HEADER_SIZE: usize = 4;
        const SAMPLES_PER_BYTE: usize = 2;

        let mut buffered_recv = BufferedStream::from_rx(rx);

        let target_chunks = full_block_size as usize * BLOCK_PER_PAYLOAD;

        let mut payload_bytes = vec![];
        let mut end_of_stream = false;
        while !end_of_stream {
            while payload_bytes.len() < target_chunks {
                let mut buffer = vec![255; target_chunks - payload_bytes.len()];
                if let Ok(read) = buffered_recv.read(&mut buffer) {
                    payload_bytes.extend(&buffer[..read]);
                } else {
                    // Error should occur if the channel is dropped
                    // and all bytes are consumed
                    end_of_stream = true;
                }
                if end_of_stream {
                    break;
                }
            }

            let mut payload = vec![];
            for block_bytes in payload_bytes.chunks(full_block_size as usize) {
                let bytes: Vec<u8> = block_bytes.to_vec();
                let bcmedia_adpcm = BcMedia::Adpcm(BcMediaAdpcm { data: bytes });
                payload = bcmedia_adpcm.serialize(payload)?;
            }

            let adpcm_len = payload_bytes.len();

            // There are two samples per byte
            //
            // To calculate the bytes we subtract the block headers from the len
            //
            // There is 1 initial sample stored in the block header so we add that in the end
            //
            let samples_sent = if adpcm_len >= BLOCK_HEADER_SIZE * BLOCK_PER_PAYLOAD {
                (adpcm_len - BLOCK_HEADER_SIZE * BLOCK_PER_PAYLOAD) * SAMPLES_PER_BYTE
                    + BLOCK_PER_PAYLOAD
            } else {
                // Zero samples in this block
                break;
            };

            payload_bytes = vec![];

            // Time to play the sample in seconds
            let play_length = samples_sent as f32 / sample_rate as f32;

            let msg = Bc {
                meta: BcMeta {
                    msg_id: MSG_ID_TALK,
                    channel_id: self.channel_id,
                    msg_num,
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

            std::thread::sleep(std::time::Duration::from_secs_f32(play_length * 0.95));
        }

        self.talk_stop()?;

        Ok(())
    }
}

struct BufferedStream {
    rx: Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    consumed: usize,
}

impl BufferedStream {
    pub fn from_rx(rx: Receiver<Vec<u8>>) -> BufferedStream {
        BufferedStream {
            rx,
            buffer: vec![],
            consumed: 0,
        }
    }
}

impl Read for BufferedStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        let buffer = self.fill_buf()?;
        let amt = std::cmp::min(buf.len(), buffer.len());

        // First check if the amount of bytes we want to read is small:
        // `copy_from_slice` will generally expand to a call to `memcpy`, and
        // for a single byte the overhead is significant.
        if amt == 1 {
            buf[0] = buffer[0];
        } else {
            buf[..amt].copy_from_slice(&buffer[..amt]);
        }

        self.consume(amt);

        Ok(amt)
    }
}

impl BufRead for BufferedStream {
    fn fill_buf(&mut self) -> IoResult<&[u8]> {
        const CLEAR_CONSUMED_AT: usize = 1024;
        // This is a trade off between caching too much dead memory
        // and calling the drain method too often
        if self.consumed > CLEAR_CONSUMED_AT {
            let _ = self.buffer.drain(0..self.consumed).collect::<Vec<u8>>();
            self.consumed = 0;
        }
        while self.buffer.len() <= self.consumed {
            let data = self
                .rx
                .recv()
                .map_err(|err| IoError::new(ErrorKind::ConnectionReset, err))?;
            self.buffer.extend(data);
        }

        Ok(&self.buffer.as_slice()[self.consumed..])
    }

    fn consume(&mut self, amt: usize) {
        assert!(self.consumed + amt <= self.buffer.len());
        self.consumed += amt;
    }
}

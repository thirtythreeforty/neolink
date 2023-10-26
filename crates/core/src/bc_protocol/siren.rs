//! Trigger for the siren

use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Trigger the siren
    pub async fn siren(&self) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection.subscribe(MSG_ID_PLAY_AUDIO, msg_num).await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PLAY_AUDIO,
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
                payload: Some(BcPayloads::BcXml(BcXml {
                    audio_play_info: Some(AudioPlayInfo {
                        channel_id: self.channel_id,
                        play_mode: 0,
                        play_duration: 0,
                        play_times: 1,
                        on_off: 0,
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_get.send(get).await?;
        let msg = sub_get.recv().await?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable(msg.meta.response_code));
        }

        Ok(())
    }
}

use futures::{StreamExt, TryStreamExt};

use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Get the snapshot image
    pub async fn get_snapshot(&self) -> Result<Vec<u8>> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection.subscribe(msg_num).await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_SNAP,
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
                    snap: Some(Snap {
                        channel_id: self.channel_id,
                        logic_channel: Some(self.channel_id),
                        time: 0,
                        full_frame: Some(0),
                        stream_type: Some("main".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_get.send(get).await?;
        let msg = sub_get.recv().await?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable);
        }

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    snap:
                        Some(Snap {
                            file_name: Some(filename),
                            picture_size: Some(expected_size),
                            ..
                        }),
                    ..
                })),
            ..
        }) = msg.body
        {
            log::trace!("Got snap {} with size {}", filename, expected_size);
            let expected_size = expected_size as usize;

            let binary_stream = sub_get.payload_stream();
            let result: Vec<_> = binary_stream
                .map_ok(|i| tokio_stream::iter(i).map(Result::Ok))
                .try_flatten()
                .take(expected_size)
                .try_collect()
                .await?;
            log::trace!("Got whole of the snap: {}", result.len());
            Ok(result)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected Snap xml but it was not recieved",
            })
        }
    }
}

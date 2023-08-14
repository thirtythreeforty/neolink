// use futures::{StreamExt, TryStreamExt};

use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Get the snapshot image
    pub async fn get_snapshot(&self) -> Result<Vec<u8>> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection.subscribe(MSG_ID_SNAP, msg_num).await?;
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
            drop(sub_get); // Ensure that we are NOT listening on that msgnum as the reply can come on ANY msgnum
            log::trace!("Got snap XML {} with size {}", filename, expected_size);
            // Messages are now sent on ID 109 but not with the same message ID
            // preumably because the camera considers it to be a new message rather
            // than a reply
            //
            // This means we need to listen for the next 109 grab the message num and
            // subscribe to it. This is what `subscribe_to_next` is for
            let mut sub_get = connection.subscribe_to_id(MSG_ID_SNAP).await?;
            let expected_size = expected_size as usize;

            let mut result: Vec<_> = vec![];
            log::trace!("Waiting for packets on {}", msg_num);
            let mut msg = sub_get.recv().await?;

            while msg.meta.response_code == 200 {
                // sends 200 while more is to come
                //       201 when finished

                if let BcBody::ModernMsg(ModernMsg {
                    extension:
                        Some(Extension {
                            binary_data: Some(1),
                            ..
                        }),
                    payload: Some(BcPayloads::Binary(data)),
                }) = msg.body
                {
                    result.extend_from_slice(&data);
                } else {
                    return Err(Error::UnintelligibleReply {
                        reply: std::sync::Arc::new(Box::new(msg)),
                        why: "Expected binary data but got something else",
                    });
                }
                log::trace!(
                    "Got packet size is now {} of {}",
                    result.len(),
                    expected_size
                );
                msg = sub_get.recv().await?;
            }

            if msg.meta.response_code == 201 {
                // 201 means all binary data sent
                if let BcBody::ModernMsg(ModernMsg {
                    extension:
                        Some(Extension {
                            binary_data: Some(1),
                            ..
                        }),
                    payload,
                }) = msg.body
                {
                    if let Some(BcPayloads::Binary(data)) = payload {
                        // Add last data if present (may be zero if preveious packet contained it)
                        result.extend_from_slice(&data);
                    }
                    log::trace!(
                        "Got all packets size is now {} of {}",
                        result.len(),
                        expected_size
                    );
                    if result.len() != expected_size {
                        log::warn!("Snap did not recieve expected number of byes");
                    }
                } else {
                    return Err(Error::UnintelligibleReply {
                        reply: std::sync::Arc::new(Box::new(msg)),
                        why: "Expected binary data but got something else",
                    });
                }
            } else {
                // anything else is an error
                return Err(Error::CameraServiceUnavaliable);
            }

            // let binary_stream = sub_get.payload_stream();
            // let result: Vec<_>= binary_stream
            //     .map_ok(|i| tokio_stream::iter(i).map(Result::Ok))
            //     .try_flatten()
            //     .take(expected_size)
            //     .try_collect()
            //     .await?;
            log::trace!("Snapshot recieved: {} of {}", result.len(), expected_size);
            Ok(result)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected Snap xml but it was not recieved",
            })
        }
    }
}

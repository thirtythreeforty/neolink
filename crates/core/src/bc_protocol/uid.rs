use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Get the [Uid] xml which contains the uid of the camera
    pub async fn get_uid(&self) -> Result<Uid> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection.subscribe(MSG_ID_UID, msg_num).await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_UID,
                channel_id: self.channel_id,
                msg_num,
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: None,
                payload: None,
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
                    uid: Some(uid_xml), ..
                })),
            ..
        }) = msg.body
        {
            Ok(uid_xml)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected Uid xml but it was not recieved",
            })
        }
    }

    /// Get the UID
    pub async fn uid(&self) -> Result<String> {
        Ok(self.get_uid().await?.uid)
    }
}

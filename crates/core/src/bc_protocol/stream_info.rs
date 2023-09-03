use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Get the [StreamInfoList] xml which contains the supported camera streams
    pub async fn get_stream_info(&self) -> Result<StreamInfoList> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection
            .subscribe(MSG_ID_STREAM_INFO_LIST, msg_num)
            .await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_STREAM_INFO_LIST,
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
                    stream_info_list: Some(data),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(data)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected StreamInfoList xml but it was not recieved",
            })
        }
    }
}

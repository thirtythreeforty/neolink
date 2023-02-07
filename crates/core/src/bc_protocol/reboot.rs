use super::{BcCamera, Error, Result};
use crate::bc::model::*;

impl BcCamera {
    /// Reboot the camera
    pub async fn reboot(&self) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub = connection.subscribe(msg_num).await?;

        let msg = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_REBOOT,
                channel_id: self.channel_id,
                msg_num,
                stream_type: 0,
                response_code: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                ..Default::default()
            }),
        };

        sub.send(msg).await?;
        let msg = sub.recv().await?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the reboot command",
            })
        }
    }
}

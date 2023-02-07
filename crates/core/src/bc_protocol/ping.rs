use super::{BcCamera, Result};
use crate::bc::model::*;

impl BcCamera {
    /// Ping the camera will either return Ok(()) which means a sucess reply
    /// or error
    pub async fn ping(&self) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_ping = connection.subscribe(msg_num).await?;

        let ping = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PING,
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

        sub_ping.send(ping).await?;

        sub_ping.recv().await?;

        Ok(())
    }
}

use super::{BcCamera, Result};
use crate::bc::model::*;

impl BcCamera {
    /// Create a handller to respond to keep alive messages
    /// These messages are sent by the camera so we listen to
    /// a message ID rather than setting a message number and
    /// responding to it
    pub async fn keepalive(&self) -> Result<()> {
        let connection = self.get_connection();
        connection
            .handle_msg(MSG_ID_UDP_KEEP_ALIVE, |bc| {
                Some(Bc {
                    meta: BcMeta {
                        msg_id: MSG_ID_UDP_KEEP_ALIVE,
                        channel_id: bc.meta.channel_id,
                        msg_num: bc.meta.msg_num,
                        stream_type: bc.meta.stream_type,
                        response_code: 200,
                        class: 0x6414,
                    },
                    body: BcBody::ModernMsg(ModernMsg {
                        ..Default::default()
                    }),
                })
            })
            .await?;
        Ok(())
    }
}

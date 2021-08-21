use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::bc::model::*;

impl BcCamera {
    /// Ping the camera will either return Ok(()) which means a sucess reply
    /// or error
    pub fn ping(&self) -> Result<()> {
        let connection = self.connection.as_ref().expect("Must be connected to ping");
        let sub_ping = connection.subscribe(MSG_ID_PING)?;

        let ping = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PING,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                stream_type: 0,
                response_code: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                ..Default::default()
            }),
        };

        sub_ping.send(ping)?;

        sub_ping.rx.recv_timeout(RX_TIMEOUT)?;

        Ok(())
    }

    /// Send a ping repeatedly
    pub fn ping_every(&self, milis: u64) -> Result<()> {
        let wait = std::time::Duration::from_millis(milis);

        let connection = self.connection.as_ref().expect("Must be connected to ping");
        let sub_ping = connection.subscribe(MSG_ID_PING)?;

        loop {
            let ping = Bc {
                meta: BcMeta {
                    msg_id: MSG_ID_PING,
                    channel_id: self.channel_id,
                    msg_num: self.new_message_num(),
                    stream_type: 0,
                    response_code: 0,
                    class: 0x6414,
                },
                body: BcBody::ModernMsg(ModernMsg {
                    ..Default::default()
                }),
            };

            sub_ping.send(ping)?;

            let reply = sub_ping.rx.recv_timeout(RX_TIMEOUT)?;

            if reply.meta.response_code != 200 {
                return Err(Error::UnintelligibleReply {
                    reply,
                    why: "Camera responded with an error status code to the ping",
                });
            }
            std::thread::sleep(wait);
        }
    }
}

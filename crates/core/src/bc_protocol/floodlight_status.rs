use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Set the floodlight status using the [FloodlightManual] xml
    pub fn set_floodlight_manual(&self, state: bool, duration: u16) -> Result<()> {
        let connection = self.get_connection();

        let msg_num = self.new_message_num();
        let sub_set = connection.subscribe(msg_num)?;

        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_FLOODLIGHT_MANUAL,
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
                    floodlight_manual: Some(FloodlightManual {
                        version: "1".to_string(),
                        channel_id: self.channel_id,
                        status: match state {
                            true => 1,
                            false => 0,
                        },
                        duration: duration,
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(get)?;
        let msg = sub_set.rx.recv_timeout(RX_TIMEOUT)?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: Box::new(msg),
                why: "The camera did not accept the Floodlight manual state",
            })
        }
    }

}

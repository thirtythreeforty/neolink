use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

/// Directions used for Ptz
pub enum Direction {
    /// To move the camera Up
    Up,
    /// To move the camera Down
    Down,
    /// To move the camera Left
    Left,
    /// To move the camera Right
    Right,
    /// To zoom the camera In (may be done with cropping depending on camera model)
    In,
    /// To zoom the camera Out (may be done with cropping depending on camera model)
    Out,
}

impl BcCamera {
    /// Send a PTZ message to the camera
    pub async fn send_ptz(&self, direction: Direction, amount: f32) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_set = connection.subscribe(msg_num).await?;

        let direction_str = match direction {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::In => {
                todo!()
            }
            Direction::Out => {
                todo!()
            }
        }
        .to_string();
        let send = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PTZ_CONTROL,
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
                    ptz_control: Some(PtzControl {
                        version: xml_ver(),
                        channel_id: self.channel_id,
                        speed: amount,
                        command: direction_str,
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(send).await?;
        let msg = sub_set.recv().await?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the PtzControl xml",
            })
        }
    }
}

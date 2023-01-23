use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Get the [RfAlarmCfg] xml which contains the PIR status of the camera
    pub fn get_pirstate(&self) -> Result<RfAlarmCfg> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let sub_get = connection.subscribe(msg_num)?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_GET_PIR_ALARM,
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
                payload: None,
            }),
        };

        sub_get.send(get)?;
        let msg = sub_get.rx.recv_timeout(RX_TIMEOUT)?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable);
        }

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    rf_alarm_cfg: Some(pirstate),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(pirstate)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected PirSate xml but it was not recieved",
            })
        }
    }

    /// Set the PIR sensor using the [RfAlarmCfg] xml
    pub fn set_pirstate(&self, rf_alarm_cfg: RfAlarmCfg) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let sub_set = connection.subscribe(msg_num)?;

        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_START_PIR_ALARM,
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
                    rf_alarm_cfg: Some(rf_alarm_cfg),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(get)?;
        let msg = sub_set.rx.recv_timeout(RX_TIMEOUT)?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable);
        }

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not except the RfAlarmCfg xml",
            })
        }
    }

    /// This is a convience function to control the PIR status
    /// True is on and false is off
    pub fn pir_set(&self, state: bool) -> Result<()> {
        let mut pir_state = self.get_pirstate()?;
        // println!("{:?}", pir_state);
        pir_state.enable = match state {
            true => 1,
            false => 0,
        };
        self.set_pirstate(pir_state)?;
        Ok(())
    }
}

/// Turn PIR ON or OFF
pub enum PirState {
    /// Turn the PIR on
    On,
    /// Turn the PIR off
    Off,
}

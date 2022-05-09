use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Get the [RfAlarmCfg] xml which contains the motion status of the camera
    pub fn get_motionstate(&mut self) -> Result<RfAlarmCfg> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to get time");
        let sub_get = connection.subscribe(MSG_ID_GET_MOTION_ALARM)?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_GET_MOTION_ALARM,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
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

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    rf_alarm_cfg: Some(motionstate),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(motionstate)
        } else {
            Err(Error::UnintelligibleReply {
                reply: msg,
                why: "Expected MotionSate xml but it was not recieved",
            })
        }
    }

    /// Set the PIR sensor using the [RfAlarmCfg] xml
    pub fn set_motionstate(&mut self, rf_alarm_cfg: RfAlarmCfg) -> Result<()> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to get time");
        let sub_set = connection.subscribe(MSG_ID_START_MOTION_ALARM)?;

        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_START_MOTION_ALARM,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
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

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: msg,
                why: "The camera did not except the RfAlarmCfg xml",
            })
        }
    }


    
    /// This is a convience function to control the PIR status
    /// True is on and false is off
    pub fn motion_set(&mut self, state: bool) -> Result<()> {
        let mut motion_state = self.get_motionstate()?;
        // println!("{:?}", motion_state);
        motion_state.enable = match state {
            true => 1,
            false => 0,
        };
        self.set_motionstate(motion_state)?;
        Ok(())
    }
}

/// Turn PIR ON or OFF
pub enum MotionState {
    /// Turn the PIR on
    On,
    /// Turn the PIR off
    Off,    
}

use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};
use tokio::time::{interval, Duration};

impl BcCamera {
    /// Get the [RfAlarmCfg] xml which contains the PIR status of the camera
    pub async fn get_pirstate(&self) -> Result<RfAlarmCfg> {
        self.has_ability_ro("rfAlarm").await?;
        let connection = self.get_connection();
        let mut reties: usize = 0;
        let mut retry_interval = interval(Duration::from_millis(500));
        loop {
            retry_interval.tick().await;
            let msg_num = self.new_message_num();
            let mut sub_get = connection.subscribe(MSG_ID_GET_PIR_ALARM, msg_num).await?;
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
                        rf_id: Some(self.channel_id),
                        ..Default::default()
                    }),
                    payload: None,
                }),
            };

            sub_get.send(get).await?;
            let msg = sub_get.recv().await?;
            if msg.meta.response_code == 400 {
                // Retryable
                if reties < 5 {
                    reties += 1;
                    continue;
                } else {
                    return Err(Error::CameraServiceUnavaliable);
                }
            } else if msg.meta.response_code != 200 {
                return Err(Error::CameraServiceUnavaliable);
            } else {
                // Valid message with response_code == 200
                if let BcBody::ModernMsg(ModernMsg {
                    payload:
                        Some(BcPayloads::BcXml(BcXml {
                            rf_alarm_cfg: Some(pirstate),
                            ..
                        })),
                    ..
                }) = msg.body
                {
                    return Ok(pirstate);
                } else {
                    return Err(Error::UnintelligibleReply {
                        reply: std::sync::Arc::new(Box::new(msg)),
                        why: "Expected PirSate xml but it was not recieved",
                    });
                }
            }
        }
    }

    /// Set the PIR sensor using the [RfAlarmCfg] xml
    pub async fn set_pirstate(&self, rf_alarm_cfg: RfAlarmCfg) -> Result<()> {
        self.has_ability_rw("rfAlarm").await?;
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_set = connection
            .subscribe(MSG_ID_START_PIR_ALARM, msg_num)
            .await?;

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
                    rf_id: Some(self.channel_id),
                    ..Default::default()
                }),
                payload: Some(BcPayloads::BcXml(BcXml {
                    rf_alarm_cfg: Some(rf_alarm_cfg),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(get).await?;
        if let Ok(reply) =
            tokio::time::timeout(tokio::time::Duration::from_micros(500), sub_set.recv()).await
        {
            let msg = reply?;
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
        } else {
            // Some cameras seem to just not send a reply on success, so after 500ms we return Ok
            Ok(())
        }
    }

    /// This is a convience function to control the PIR status
    /// True is on and false is off
    pub async fn pir_set(&self, state: bool) -> Result<()> {
        let mut pir_state = self.get_pirstate().await?;
        // println!("{:?}", pir_state);
        pir_state.enable = match state {
            true => 1,
            false => 0,
        };
        self.set_pirstate(pir_state).await?;
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

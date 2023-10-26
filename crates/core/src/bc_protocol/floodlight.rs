use tokio::sync::mpsc::{channel, Receiver};

use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Listen on the flood light update messages and return their XMLs
    pub async fn listen_on_flightlight(&self) -> Result<Receiver<FloodlightStatusList>> {
        let (tx, rx) = channel(3);
        let connection = self.get_connection();
        connection
            .handle_msg(MSG_ID_FLOODLIGHT_STATUS_LIST, move |bc| {
                let tx = tx.clone();
                Box::pin(async move {
                    if let Bc {
                        meta:
                            BcMeta {
                                msg_id: MSG_ID_FLOODLIGHT_STATUS_LIST,
                                ..
                            },
                        body:
                            BcBody::ModernMsg(ModernMsg {
                                payload:
                                    Some(BcPayloads::BcXml(BcXml {
                                        floodlight_status_list: Some(list),
                                        ..
                                    })),
                                ..
                            }),
                    } = bc
                    {
                        let send_this: FloodlightStatusList = list.clone();
                        let _ = tx.send(send_this).await;
                    }
                    None
                })
            })
            .await?;

        Ok(rx)
    }

    /// Set the floodlight status using the [FloodlightManual] xml
    pub async fn set_floodlight_manual(&self, state: bool, duration: u16) -> Result<()> {
        let connection = self.get_connection();

        let msg_num = self.new_message_num();
        let mut sub_set = connection
            .subscribe(MSG_ID_FLOODLIGHT_MANUAL, msg_num)
            .await?;

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
                        duration,
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(get).await?;

        if let Ok(reply) =
            tokio::time::timeout(tokio::time::Duration::from_micros(500), sub_set.recv()).await
        {
            let msg = reply?;
            if let BcMeta {
                response_code: 200, ..
            } = msg.meta
            {
                Ok(())
            } else {
                Err(Error::UnintelligibleReply {
                    reply: std::sync::Arc::new(Box::new(msg)),
                    why: "The camera did not accept the Floodlight manual state",
                })
            }
        } else {
            // Some cameras seem to just not send a reply on success, so after 500ms we return Ok
            Ok(())
        }
    }

    /// Get the Flood Light tasks XML
    pub async fn get_flightlight_tasks(&self) -> Result<FloodlightTask> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection
            .subscribe(MSG_ID_FLOODLIGHT_TASKS_READ, msg_num)
            .await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_FLOODLIGHT_TASKS_READ,
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

        sub_get.send(get).await?;
        let msg = sub_get.recv().await?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable(msg.meta.response_code));
        }

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    floodlight_task: Some(xml),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(xml)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected FloodlightTask xml but it was not recieved",
            })
        }
    }

    /// Set the Flood Light tasks XML
    pub async fn set_flightlight_tasks(&self, new_xml: FloodlightTask) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection
            .subscribe(MSG_ID_FLOODLIGHT_TASKS_WRITE, msg_num)
            .await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_FLOODLIGHT_TASKS_WRITE,
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
                    floodlight_task: Some(new_xml),
                    ..Default::default()
                })),
            }),
        };

        sub_get.send(get).await?;
        let msg = sub_get.recv().await?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable(msg.meta.response_code));
        }

        Ok(())
    }

    /// Convience function: Activate the Flood Light night mode
    pub async fn flightlight_tasks_enable(&self, state: bool) -> Result<()> {
        // println!("{:?}", pir_state);
        if self.is_flightlight_tasks_enabled().await? != state {
            let mut curr_state = self.get_flightlight_tasks().await?;
            curr_state.enable = match state {
                true => 1,
                false => 0,
            };
            self.set_flightlight_tasks(curr_state).await?;
        }
        Ok(())
    }

    /// Convience function: Check if Flood Light tasks are enbabled
    pub async fn is_flightlight_tasks_enabled(&self) -> Result<bool> {
        let curr_state = self.get_flightlight_tasks().await?;
        Ok(curr_state.enable == 1)
    }
}

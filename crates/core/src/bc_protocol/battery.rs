//! Handles battery related messages
//!
//! There are primarily two messages:
//! - BatteryInfoList which the camera sends as part of its login info
//! - BatteryInfo which the client can request on demand
//!

use super::{BcCamera, PrintFormat, Result};
use crate::{
    bc::{model::*, xml::BatteryInfo},
    Error,
};

impl BcCamera {
    /// Create a handller to respond to battery messages
    /// These messages are sent by the camera on login and maybe
    /// also on low battery events
    pub async fn monitor_battery(&self, format: PrintFormat) -> Result<()> {
        let connection = self.get_connection();
        connection
            .handle_msg(MSG_ID_BATTERY_INFO_LIST, move |bc| {
                Box::pin(async move {
                    if let Bc {
                        body:
                            BcBody::ModernMsg(ModernMsg {
                                payload:
                                    Some(BcPayloads::BcXml(BcXml {
                                        battery_list: Some(battery_list),
                                        ..
                                    })),
                                ..
                            }),
                        ..
                    } = bc
                    {
                        for battery in battery_list.battery_info.iter() {
                            match format {
                                PrintFormat::None => {}
                                PrintFormat::Human => {
                                    println!(
                                        "==Battery==\n\
                                    Charge: {}%,\n\
                                    Temperature: {}°C,\n\
                                    LowPower: {},\n\
                                    Adapter: {},\n\
                                    ChargeStatus: {},\n\
                                    ",
                                        battery.battery_percent,
                                        battery.temperature,
                                        if battery.low_power == 1 {
                                            "true"
                                        } else {
                                            "false"
                                        },
                                        battery.adapter_status,
                                        battery.charge_status,
                                    );
                                }
                                PrintFormat::Xml => {
                                    let bat_ser = String::from_utf8(
                                        yaserde::ser::serialize_with_writer(
                                            battery,
                                            vec![],
                                            &Default::default(),
                                        )
                                        .expect("Should Ser the struct"),
                                    )
                                    .expect("Should be UTF8");
                                    println!("{}", bat_ser);
                                }
                            }
                        }
                    }
                    Option::<Bc>::None
                })
            })
            .await?;
        Ok(())
    }

    /// Requests the current battery status of the camera
    pub async fn battery_info(&self) -> Result<BatteryInfo> {
        let connection = self.get_connection();

        let msg_num = self.new_message_num();
        let mut sub = connection.subscribe(MSG_ID_BATTERY_INFO, msg_num).await?;

        let msg = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_BATTERY_INFO,
                channel_id: self.channel_id,
                msg_num,
                stream_type: 0,
                response_code: 0,
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

        sub.send(msg).await?;
        let msg = sub.recv().await?;

        if let Bc {
            meta: BcMeta {
                response_code: 200, ..
            },
            body:
                BcBody::ModernMsg(ModernMsg {
                    payload:
                        Some(BcPayloads::BcXml(BcXml {
                            battery_info: Some(battery_info),
                            ..
                        })),
                    ..
                }),
        } = msg
        {
            Ok(battery_info)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the battery info (maybe no battery) command.",
            })
        }
    }
}

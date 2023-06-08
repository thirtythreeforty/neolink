use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};
use log::*;

impl BcCamera {
    /// Get the ability info xml for the current user
    pub async fn get_abilityinfo(&self) -> Result<AbilityInfo> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection.subscribe(MSG_ID_ABILITY_INFO, msg_num).await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_ABILITY_INFO,
                channel_id: self.channel_id,
                msg_num,
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(Extension {
                    user_name: Some(self.get_credentials().username.clone()),
                    token: Some("system, streaming, PTZ, IO, security, replay, disk,  network, alarm, record, video, image".to_string()),
                    ..Default::default()
                }),
                payload: None,
            }),
        };

        sub_get.send(get).await?;
        let msg = sub_get.recv().await?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable);
        }

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    ability_info: Some(ability_info),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(ability_info)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected AbilityInfo xml but it was not recieved",
            })
        }
    }

    /// Populate ability list of the camera
    pub async fn polulate_abilities(&self) -> Result<()> {
        let info = self.get_abilityinfo().await?;
        let info_res = yaserde::ser::serialize_with_writer(&info, vec![], &Default::default());
        if let Ok(Ok(info_str)) = info_res.map(String::from_utf8) {
            debug!("Abilities: {}", info_str);
        }

        let mut abilities: Vec<String> = vec![];

        let mut tokens: Vec<Option<&AbilityInfoToken>> = vec![
            info.system.as_ref(),
            info.network.as_ref(),
            info.alarm.as_ref(),
            info.image.as_ref(),
            info.video.as_ref(),
            info.security.as_ref(),
            info.replay.as_ref(),
            info.ptz.as_ref(),
            info.io.as_ref(),
            info.streaming.as_ref(),
        ];

        for token in tokens.drain(..).flatten() {
            for sub_module in token.sub_module.iter() {
                abilities.extend(
                    sub_module
                        .ability_value
                        .replace(' ', "")
                        .split(',')
                        .map(|s| s.to_string()),
                );
            }
        }

        let mut locked_abilities = self.abilities.write().await;
        for ability in abilities.iter() {
            let mut abilities_ro = ability.split('_').map(|s| s.to_string());
            if let (Some(ability_name), Some(ability_kind)) =
                (abilities_ro.next(), abilities_ro.next())
            {
                match ability_kind.as_str() {
                    "rw" => {
                        locked_abilities.insert(ability_name, super::ReadKind::ReadWrite);
                    }
                    "ro" => {
                        locked_abilities.insert(ability_name, super::ReadKind::ReadOnly);
                    }
                    _ => {
                        continue;
                    }
                }
            }
        }

        Ok(())
    }
}

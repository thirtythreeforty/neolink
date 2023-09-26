use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

/// Specifies the phone type for the push notification
pub enum PhoneType {
    /// Specify that this is an ios push notfication
    ///
    /// In this case the token must be the APNS
    Ios,
    /// Specify that this is an andriod push notfication
    ///
    /// In this case the token must firebase cloud messaging token
    Android,
}

impl BcCamera {
    /// Convenience method for andriod of `[send_pushinfo]`
    pub async fn send_pushinfo_android(&self, token: &str, client_id: &str) -> Result<()> {
        self.send_pushinfo(token, client_id, PhoneType::Android)
            .await
    }
    /// Convenience method for andriod of `[send_pushinfo]`
    pub async fn send_pushinfo_ios(&self, token: &str, client_id: &str) -> Result<()> {
        self.send_pushinfo(token, client_id, PhoneType::Ios).await
    }
    /// Send the push info to regsiter for push notfications
    pub async fn send_pushinfo(
        &self,
        token: &str,
        client_id: &str,
        phone_type: PhoneType,
    ) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub = connection.subscribe(MSG_ID_PUSH_INFO, msg_num).await?;

        let phone_type_str = match phone_type {
            PhoneType::Ios => "reo_iphone",
            PhoneType::Android => "reo_fcm",
        };

        let msg = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PUSH_INFO,
                channel_id: self.channel_id,
                msg_num,
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: None,
                payload: Some(BcPayloads::BcXml(BcXml {
                    push_info: Some(PushInfo {
                        token: token.to_owned(),
                        phone_type: phone_type_str.to_owned(),
                        client_id: client_id.to_owned(),
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub.send(msg).await?;
        let msg = sub.recv().await?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable);
        }

        Ok(())
    }
}

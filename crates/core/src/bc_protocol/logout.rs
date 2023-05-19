use super::{BcCamera, Result};
use crate::bc::{model::*, xml::*};
use std::sync::atomic::Ordering;

impl BcCamera {
    /// Logout from the camera
    pub async fn logout(&self) -> Result<()> {
        if self.logged_in.load(Ordering::Relaxed) {
            let credentials = self.get_credentials();
            let connection = self.get_connection();
            let msg_num = self.new_message_num();
            let sub_logout = connection.subscribe(msg_num).await?;

            let username = credentials.username.clone();
            let password = credentials.password.as_ref().cloned().unwrap_or_default();

            let modern_logout = Bc::new_from_xml(
                BcMeta {
                    msg_id: MSG_ID_LOGOUT,
                    channel_id: self.channel_id,
                    msg_num,
                    stream_type: 0,
                    response_code: 0,
                    class: 0x6414,
                },
                BcXml {
                    login_user: Some(LoginUser {
                        version: xml_ver(),
                        user_name: username,
                        password,
                        user_ver: 1,
                    }),
                    login_net: Some(LoginNet::default()),
                    ..Default::default()
                },
            );

            sub_logout.send(modern_logout).await?;
        }
        self.logged_in.store(false, Ordering::Relaxed);
        Ok(())
    }
}

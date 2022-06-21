use super::{BcCamera, Result};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Logout from the camera
    pub fn logout(&mut self) -> Result<()> {
        if self.logged_in {
            if let Some(credentials) = self.get_credentials() {
                let connection = self
                    .connection
                    .as_ref()
                    .expect("Must be connected to log in");
                let msg_num = self.new_message_num();
                let sub_logout = connection.subscribe(msg_num)?;

                let username = credentials.username.clone();
                let password = credentials
                    .password
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| "".to_string());

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

                sub_logout.send(modern_logout)?;
            }
        }
        self.clear_credentials();
        self.logged_in = false;
        Ok(())
    }
}

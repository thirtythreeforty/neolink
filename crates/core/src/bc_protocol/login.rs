use super::{md5_string, BcCamera, Error, Result, Truncate, ZeroLast};
use crate::bc::{model::*, xml::*};
use std::sync::atomic::Ordering;

/// The requested encryption level to request
/// to the camera
///
/// The camera may use a lower one depending on support
///
/// Note the reolink camera only encrypt the control messages
/// the camera feed is always accessible
#[derive(Debug, Clone, Copy)]
pub enum MaxEncryption {
    /// No encryption
    None,
    /// BCEncrypt is a simple XOR algortirhm with a fixed key
    /// used in many older models
    BcEncrypt,
    /// AES is used in newer model
    Aes,
}

impl BcCamera {
    /// Login to the camera.
    ///
    /// This should be called before most other commands
    pub async fn login(&self) -> Result<DeviceInfo> {
        self.login_with_maxenc(MaxEncryption::Aes).await
    }
    /// Login to the camera.
    ///
    /// This should be called before most other commands
    pub async fn login_with_maxenc(&self, max_encryption: MaxEncryption) -> Result<DeviceInfo> {
        let device_info;
        // This { is here due to the connection and set_credentials both requiring a mutable borrow
        {
            let credentials = self.get_credentials();
            let connection = self.get_connection();
            let msg_num = self.new_message_num();
            let mut sub_login = connection.subscribe(msg_num).await?;

            // Login flow is: Send legacy login message, expect back a modern message with Encryption
            // details.  Then, re-send the login as a modern login message.  Expect back a device info
            // congratulating us on logging in.

            // In the legacy scheme, username/password are MD5'd if they are encrypted (which they need
            // to be to "upgrade" to the modern login flow), then the hex of the MD5 is sent.
            // Note: I suspect there may be a buffer overflow opportunity in the firmware since in the
            // Baichuan library, these strings are capped at 32 bytes with a null terminator.  This
            // could also be a mistake in the library, the effect being it only compares 31 chars, not 32.
            let md5_username = md5_string(&credentials.username, ZeroLast);
            let md5_password = credentials
                .password
                .as_ref()
                .map(|p| md5_string(p, ZeroLast))
                .unwrap_or_else(|| EMPTY_LEGACY_PASSWORD.to_owned());

            let enc_byte = match max_encryption {
                MaxEncryption::None => 0xdc00,
                MaxEncryption::BcEncrypt => 0xdc01,
                MaxEncryption::Aes => 0xdc02,
            };
            let legacy_login = Bc {
                meta: BcMeta {
                    msg_id: MSG_ID_LOGIN,
                    channel_id: self.channel_id,
                    msg_num,
                    stream_type: 0,
                    response_code: enc_byte,
                    class: 0x6514,
                },
                body: BcBody::LegacyMsg(LegacyMsg::LoginMsg {
                    username: md5_username,
                    password: md5_password,
                }),
            };

            sub_login.send(legacy_login).await?;

            let legacy_reply = sub_login.recv().await?;

            let nonce;
            match &legacy_reply.body {
                BcBody::ModernMsg(ModernMsg {
                    payload:
                        Some(BcPayloads::BcXml(BcXml {
                            encryption: Some(encryption),
                            ..
                        })),
                    ..
                }) => {
                    nonce = &encryption.nonce;
                }
                _ => {
                    return Err(Error::UnintelligibleReply {
                        reply: std::sync::Arc::new(Box::new(legacy_reply)),
                        why: "Expected an Encryption message back",
                    })
                }
            }

            // In the modern login flow, the username/password are concat'd with the server's nonce
            // string, then MD5'd, then the hex of this MD5 is sent as the password.  This nonce
            // prevents replay attacks if the server were to require modern flow, but not rainbow table
            // attacks (since  the plain user/password MD5s have already been sent).  The upshot is that
            // you should use a very strong random password that is not found in a rainbow table and
            // not feasibly crackable with John the Ripper.

            let modern_password = credentials.password.clone().unwrap_or_default();
            let concat_username = format!("{}{}", credentials.username, nonce);
            let concat_password = format!("{}{}", modern_password, nonce);
            let md5_username = md5_string(&concat_username, Truncate);
            let md5_password = md5_string(&concat_password, Truncate);

            let modern_login = Bc::new_from_xml(
                BcMeta {
                    msg_id: MSG_ID_LOGIN,
                    channel_id: self.channel_id,
                    msg_num,
                    stream_type: 0,
                    response_code: 0,
                    class: 0x6414,
                },
                BcXml {
                    login_user: Some(LoginUser {
                        version: xml_ver(),
                        user_name: md5_username,
                        password: md5_password,
                        user_ver: 1,
                    }),
                    login_net: Some(LoginNet::default()),
                    ..Default::default()
                },
            );

            sub_login.send(modern_login).await?;
            let modern_reply = sub_login.recv().await?;
            if modern_reply.meta.response_code != 200 {
                return Err(Error::CameraServiceUnavaliable);
            }

            match modern_reply.body {
                BcBody::ModernMsg(ModernMsg {
                    payload:
                        Some(BcPayloads::BcXml(BcXml {
                            device_info: Some(info),
                            ..
                        })),
                    ..
                }) => {
                    // Login succeeded!
                    self.logged_in.store(true, Ordering::Relaxed);
                    device_info = info;
                }
                BcBody::ModernMsg(ModernMsg {
                    extension: None,
                    payload: None,
                }) => return Err(Error::AuthFailed),
                _ => {
                    return Err(Error::UnintelligibleReply {
                        reply: std::sync::Arc::new(Box::new(legacy_reply)),
                        why: "Expected a DeviceInfo message back from login",
                    })
                }
            }
        }
        Ok(device_info)
    }
}

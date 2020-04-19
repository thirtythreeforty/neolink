use err_derive::Error;
use log::*;
use md5;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, TcpStream};
use super::bc::{model::*, xml::*};

use Md5Trunc::*;

pub struct BcCamera {
    address: SocketAddr,

    connection: Option<TcpStream>,
    logged_in: bool,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(display="Communication error")]
    CommunicationError(#[error(source)] std::io::Error),
    #[error(display="Deserialization error")]
    DeserializationError(#[error(source)] super::bc::de::Error),
    #[error(display="Serialization error")]
    SerializationError(#[error(source)] super::bc::ser::Error),
    #[error(display="Communication error")]
    UnintelligibleReply {
        request: Bc,
        reply: Bc,
        why: &'static str,
    },
    #[error(display="Credential error")]
    AuthFailed,
    #[error(display="Other error")]
    OtherError(&'static str),
}

type Result<T> = std::result::Result<T, Error>;

impl Drop for BcCamera {
    fn drop(&mut self) {
        self.disconnect();
    }
}

impl BcCamera {
    pub fn new_with_ip(ip_addr: IpAddr) -> Result<Self> {
        Self::new_with_addr((ip_addr, 9000))
    }

    pub fn new_with_addr<T: ToSocketAddrs>(hostname: T) -> Result<Self> {
        let address = hostname.to_socket_addrs()?.next()
            .ok_or(Error::OtherError("Address resolution failed"))?;

        Ok(Self {
            address,
            connection: None,
            logged_in: false,
        })
    }

    pub fn connect(&mut self) -> Result<()> {
        self.connection = Some(TcpStream::connect(self.address)?);
        Ok(())
    }

    pub fn disconnect(&mut self) {
        if let Err(err) = self.logout() {
            warn!("Could not log out, ignoring: {}", err);
        }
        self.connection = None;
    }

    pub fn login(&mut self, username: &str, password: Option<&str>) -> Result<DeviceInfo> {
        let connection = self.connection.as_ref().expect("Must be connected to log in");

        // Login flow is: Send legacy login message, expect back a modern message with Encryption
        // details.  Then, re-send the login as a modern login message.  Expect back a device info
        // congratulating us on logging in.

        // In the legacy scheme, username/password are MD5'd if they are encrypted (which they need
        // to be to "upgrade" to the modern login flow), then the hex of the MD5 is sent.
        // Note: I suspect there may be a buffer overflow opportunity in the firmware since in the
        // Baichuan library, these strings are capped at 32 bytes with a null terminator.  This
        // could also be a mistake in the library, the effect being it only compares 31 chars, not 32.
        let md5_username = md5_string(username, ZeroLast);
        let md5_password = password.map(|p| md5_string(p, ZeroLast)).unwrap_or(EMPTY_LEGACY_PASSWORD.to_owned());

        let legacy_login = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_LOGIN,
                client_idx: 0,
                encrypted: true,
                class: 0x6514,
            },
            body: BcBody::LegacyMsg(LegacyMsg::LoginMsg {
                username: md5_username,
                password: md5_password,
            })
        };

        legacy_login.serialize(connection)?;

        let legacy_reply = Bc::deserialize(connection)?;
        let nonce;
        match legacy_reply.body {
            BcBody::ModernMsg(ModernMsg {
                xml: Some(Body { encryption: Some(encryption), ..  }), ..
            }) => {
                nonce = encryption.nonce;
            }
            _ => return Err(Error::UnintelligibleReply {
                request: legacy_login,
                reply: legacy_reply,
                why: "Expected an Encryption message back"
            })
        }

        // In the modern login flow, the username/password are concat'd with the server's nonce
        // string, then MD5'd, then the hex of this MD5 is sent as the password.  This nonce
        // prevents replay attacks if the server were to require modern flow, but not rainbow table
        // attacks (since the plain user/password MD5s have already been sent).  The upshot is that
        // you should use a very strong random password that is not found in a rainbow table and
        // not feasibly crackable with John the Ripper.

        let modern_password = password.unwrap_or("");
        let concat_username = format!("{}{}", username, nonce);
        let concat_password = format!("{}{}", modern_password, nonce);
        let md5_username = md5_string(&concat_username, Truncate);
        let md5_password = md5_string(&concat_password, Truncate);

        let modern_login = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_LOGIN,
                client_idx: 0, // TODO
                encrypted: true,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                xml: Some(Body {
                    login_user: Some(LoginUser {
                        version: xml_ver(),
                        user_name: md5_username,
                        password: md5_password,
                        user_ver: 1,
                    }),
                    login_net: Some(LoginNet::default()),
                    ..Default::default()
                }),
                binary: None,
            }),
        };

        modern_login.serialize(connection)?;

        let modern_reply = Bc::deserialize(connection)?;
        let device_info;
        match modern_reply.body {
            BcBody::ModernMsg(ModernMsg {
                xml: Some(Body { device_info: Some(info), ..  }), ..
            }) => {
                // Login succeeded!
                self.logged_in = true;
                device_info = info;
            }
            BcBody::ModernMsg(ModernMsg {
                xml: None, binary: None,
            }) => {
                return Err(Error::AuthFailed)
            }
            _ => return Err(Error::UnintelligibleReply {
                request: modern_login,
                reply: modern_reply,
                why: "Expected a DeviceInfo message back from login"
            })
        }

        Ok(device_info)
    }

    pub fn logout(&mut self) -> Result<()> {
        if self.logged_in {
            // TODO
        }
        self.logged_in = false;
        Ok(())
    }
}

/// The Baichuan library has a very peculiar behavior where it always zeros the last byte.  I
/// believe this is because the MD5'ing of the user/password is a recent retrofit to the code and
/// the original code wanted to prevent a buffer overflow with strcpy.  The modern and legacy login
/// messages have a slightly different behavior; the legacy message has a 32-byte buffer and the
/// modern message uses XML.  The legacy code copies all 32 bytes with memcpy, and the XML value is
/// copied from a C-style string, so the appended null byte is dropped by the XML library - see the
/// test below.
/// Emulate this behavior by providing a configurable mangling of the last character.
#[derive(PartialEq, Eq)]
enum Md5Trunc {
    ZeroLast,
    Truncate
}

fn md5_string(input: &str, trunc: Md5Trunc) -> String {
    let mut md5 = format!("{:X}\0", md5::compute(input));
    md5.replace_range(31.., if trunc == Truncate { "" } else { "\0" });
    md5
}

#[test]
fn test_md5_string() {
    // Note that these literals are only 31 characters long - see explanation above.
    assert_eq!(md5_string("admin", Truncate), "21232F297A57A5A743894A0E4A801FC");
    assert_eq!(md5_string("admin", ZeroLast), "21232F297A57A5A743894A0E4A801FC\0");
}

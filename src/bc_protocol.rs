use self::connection::BcConnection;
use self::media_packet::{MediaDataKind, MediaDataSubscriber};
use crate::bc;
use crate::bc::{model::*, xml::*};
use crate::gst::GstOutputs;
use adpcm::adpcm_to_pcm;
use err_derive::Error;
use log::*;
use std::convert::TryInto;
use std::io::Write;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use Md5Trunc::*;

mod adpcm;
mod connection;
mod media_packet;
mod time;

pub struct BcCamera {
    address: SocketAddr,
    channel_id: u8,
    connection: Option<BcConnection>,
    logged_in: bool,
    message_num: AtomicU16,
}

use crate::Never;

type Result<T> = std::result::Result<T, Error>;

const RX_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(display = "Communication error")]
    CommunicationError(#[error(source)] std::io::Error),

    #[error(display = "Deserialization error")]
    DeserializationError(#[error(source)] bc::de::Error),

    #[error(display = "Serialization error")]
    SerializationError(#[error(source)] bc::ser::Error),

    #[error(display = "Connection error")]
    ConnectionError(#[error(source)] self::connection::Error),

    #[error(display = "Communication error")]
    UnintelligibleReply { reply: Bc, why: &'static str },

    #[error(display = "Dropped connection")]
    DroppedConnection(#[error(source)] std::sync::mpsc::RecvError),

    #[error(display = "Timeout")]
    Timeout,

    #[error(display = "Dropped connection")]
    TimeoutDisconnected,

    #[error(display = "Credential error")]
    AuthFailed,

    #[error(display = "Failed to translate camera address")]
    AddrResolutionError,

    #[error(display = "ADPCM Decoding Error")]
    AdpcmDecodingError(&'static str),

    #[error(display = "Other error")]
    Other(&'static str),
}

impl<'a> From<std::sync::mpsc::RecvTimeoutError> for Error {
    fn from(k: std::sync::mpsc::RecvTimeoutError) -> Self {
        match k {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::Timeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDisconnected,
        }
    }
}

impl Drop for BcCamera {
    fn drop(&mut self) {
        self.disconnect();
    }
}

impl BcCamera {
    pub fn new_with_addr<T: ToSocketAddrs>(host: T, channel_id: u8) -> Result<Self> {
        let addr_iter = match host.to_socket_addrs() {
            Ok(iter) => iter,
            Err(_) => return Err(Error::AddrResolutionError),
        };

        for addr in addr_iter {
            debug!("Trying {}", addr);
            let conn = match BcConnection::new(addr, RX_TIMEOUT) {
                Ok(conn) => conn,
                Err(err) => match err {
                    connection::Error::CommunicationError(ref err) => {
                        debug!("Assuming timeout from {}", err);
                        continue;
                    }
                    err => return Err(err.into()),
                },
            };

            debug!("Success: {}", addr);
            return Ok(Self {
                address: addr,
                connection: Some(conn),
                message_num: AtomicU16::new(0),
                channel_id,
                logged_in: false,
            });
        }

        Err(Error::Timeout)
    }

    pub fn new_message_num(&self) -> u16 {
        self.message_num.fetch_add(1, Ordering::Relaxed)
    }

    pub fn disconnect(&mut self) {
        if let Err(err) = self.logout() {
            warn!("Could not log out, ignoring: {}", err);
        }
        self.connection = None;
    }

    pub fn login(&mut self, username: &str, password: Option<&str>) -> Result<DeviceInfo> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to log in");
        let sub_login = connection.subscribe(MSG_ID_LOGIN)?;

        // Login flow is: Send legacy login message, expect back a modern message with Encryption
        // details.  Then, re-send the login as a modern login message.  Expect back a device info
        // congratulating us on logging in.

        // In the legacy scheme, username/password are MD5'd if they are encrypted (which they need
        // to be to "upgrade" to the modern login flow), then the hex of the MD5 is sent.
        // Note: I suspect there may be a buffer overflow opportunity in the firmware since in the
        // Baichuan library, these strings are capped at 32 bytes with a null terminator.  This
        // could also be a mistake in the library, the effect being it only compares 31 chars, not 32.
        let md5_username = md5_string(username, ZeroLast);
        let md5_password = password
            .map(|p| md5_string(p, ZeroLast))
            .unwrap_or_else(|| EMPTY_LEGACY_PASSWORD.to_owned());

        let legacy_login = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_LOGIN,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                stream_type: 0,
                response_code: 0xdc03,
                class: 0x6514,
            },
            body: BcBody::LegacyMsg(LegacyMsg::LoginMsg {
                username: md5_username,
                password: md5_password,
            }),
        };

        sub_login.send(legacy_login)?;

        let legacy_reply = sub_login.rx.recv_timeout(RX_TIMEOUT)?;

        let nonce;
        match legacy_reply.body {
            BcBody::ModernMsg(ModernMsg {
                payload:
                    Some(BcPayloads::BcXml(BcXml {
                        encryption: Some(encryption),
                        ..
                    })),
                ..
            }) => {
                nonce = encryption.nonce;
            }
            _ => {
                return Err(Error::UnintelligibleReply {
                    reply: legacy_reply,
                    why: "Expected an Encryption message back",
                })
            }
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

        let modern_login = Bc::new_from_xml(
            BcMeta {
                msg_id: MSG_ID_LOGIN,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
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

        sub_login.send(modern_login)?;
        let modern_reply = sub_login.rx.recv_timeout(RX_TIMEOUT)?;

        let device_info;
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
                self.logged_in = true;
                device_info = info;
            }
            BcBody::ModernMsg(ModernMsg {
                extension: None,
                payload: None,
            }) => return Err(Error::AuthFailed),
            _ => {
                return Err(Error::UnintelligibleReply {
                    reply: modern_reply,
                    why: "Expected a DeviceInfo message back from login",
                })
            }
        }

        if let EncryptionProtocol::Aes(_) = connection.get_encrypted() {
            // We setup the data for the AES key now
            // as all subsequent communications will use it
            let passwd = password.unwrap_or("");
            let full_key = make_aes_key(&nonce, passwd);
            connection.set_encrypted(EncryptionProtocol::Aes(Some(full_key)));
        }

        Ok(device_info)
    }

    pub fn logout(&mut self) -> Result<()> {
        if self.logged_in {
            // TODO: Send message ID 2
        }
        self.logged_in = false;
        Ok(())
    }

    pub fn version(&self) -> Result<VersionInfo> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to get version info");
        let sub_version = connection.subscribe(MSG_ID_VERSION)?;

        let version = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_VERSION,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                stream_type: 0,
                response_code: 0,
                class: 0x6414, // IDK why
            },
            body: BcBody::ModernMsg(ModernMsg {
                ..Default::default()
            }),
        };

        sub_version.send(version)?;

        let modern_reply = sub_version.rx.recv_timeout(RX_TIMEOUT)?;
        let version_info;
        match modern_reply.body {
            BcBody::ModernMsg(ModernMsg {
                payload:
                    Some(BcPayloads::BcXml(BcXml {
                        version_info: Some(info),
                        ..
                    })),
                ..
            }) => {
                version_info = info;
            }
            _ => {
                return Err(Error::UnintelligibleReply {
                    reply: modern_reply,
                    why: "Expected a VersionInfo message",
                })
            }
        }

        Ok(version_info)
    }

    pub fn ping(&self) -> Result<()> {
        let connection = self.connection.as_ref().expect("Must be connected to ping");
        let sub_ping = connection.subscribe(MSG_ID_PING)?;

        let ping = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PING,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                stream_type: 0,
                response_code: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                ..Default::default()
            }),
        };

        sub_ping.send(ping)?;

        sub_ping.rx.recv_timeout(RX_TIMEOUT)?;

        Ok(())
    }

    pub fn start_video(&self, data_outs: &mut GstOutputs, stream_name: &str) -> Result<Never> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to start video");
        let sub_video = connection.subscribe(MSG_ID_VIDEO)?;

        let stream_num = match stream_name {
            "mainStream" => 0,
            "subStream" => 1,
            _ => 0,
        };

        let start_video = Bc::new_from_xml(
            BcMeta {
                msg_id: MSG_ID_VIDEO,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                stream_type: stream_num,
                response_code: 0,
                class: 0x6414, // IDK why
            },
            BcXml {
                preview: Some(Preview {
                    version: xml_ver(),
                    channel_id: self.channel_id,
                    handle: 0,
                    stream_type: stream_name.to_string(),
                }),
                ..Default::default()
            },
        );

        sub_video.send(start_video)?;

        let mut media_sub = MediaDataSubscriber::from_bc_sub(&sub_video);

        loop {
            let binary_data = media_sub.next_media_packet()?;
            // We now have a complete interesting packet. Send it to gst.
            // Process the packet
            match binary_data.kind() {
                MediaDataKind::VideoDataIframe | MediaDataKind::VideoDataPframe => {
                    let media_format = binary_data.media_format();
                    data_outs.set_format(media_format);
                    data_outs.vidsrc.write_all(binary_data.body())?;
                }
                MediaDataKind::AudioDataAac => {
                    let media_format = binary_data.media_format();
                    data_outs.set_format(media_format);
                    data_outs.audsrc.write_all(binary_data.body())?;
                }
                MediaDataKind::AudioDataAdpcm => {
                    let media_format = binary_data.media_format();
                    data_outs.set_format(media_format);
                    let adpcm = binary_data.body();
                    let pcm = adpcm_to_pcm(adpcm)?;
                    data_outs.audsrc.write_all(&pcm)?;
                }
                _ => {}
            };
        }
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
    Truncate,
}

fn md5_string(input: &str, trunc: Md5Trunc) -> String {
    let mut md5 = format!("{:X}\0", md5::compute(input));
    md5.replace_range(31.., if trunc == Truncate { "" } else { "\0" });
    md5
}

pub fn make_aes_key(nonce: &str, passwd: &str) -> [u8; 16] {
    let key_phrase = format!("{}-{}", nonce, passwd);
    let key_phrase_hash = format!("{:X}\0", md5::compute(&key_phrase))
        .to_uppercase()
        .into_bytes();
    key_phrase_hash[0..16].try_into().unwrap()
}

#[test]
fn test_md5_string() {
    // Note that these literals are only 31 characters long - see explanation above.
    assert_eq!(
        md5_string("admin", Truncate),
        "21232F297A57A5A743894A0E4A801FC"
    );
    assert_eq!(
        md5_string("admin", ZeroLast),
        "21232F297A57A5A743894A0E4A801FC\0"
    );
}

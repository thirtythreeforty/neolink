use self::connection::BcConnection;
use self::media_packet::{MediaDataKind, MediaDataSubscriber};
pub use self::motion::{MotionDataSubscriber, MotionStatus};
use crate::bc;
use crate::bc::{model::*, xml::*};
use crossbeam_channel::Sender;
use err_derive::Error;
use log::*;
use std::io::Write;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use Md5Trunc::*;

mod connection;
mod media_packet;
mod motion;
mod time;

pub struct BcCamera {
    address: SocketAddr,
    connection: Option<BcConnection>,
    logged_in: bool,
}

use crate::Never;

type Result<T> = std::result::Result<T, Error>;

const RX_TIMEOUT: Duration = Duration::from_secs(500);

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
    Timeout(#[error(source)] std::sync::mpsc::RecvTimeoutError),

    // We map std::sync::mpsc::RecvTimeoutError onto one of these based on
    // the errors enum
    #[error(display = "Dropped connection")]
    TimeoutDropped,

    #[error(display = "Timeout")]
    TimeoutTimeout,

    #[error(display = "Credential error")]
    AuthFailed,

    #[error(display = "Other error")]
    Other(&'static str),
}

impl Drop for BcCamera {
    fn drop(&mut self) {
        self.disconnect();
    }
}

impl BcCamera {
    pub fn new_with_addr<T: ToSocketAddrs>(hostname: T) -> Result<Self> {
        let address = hostname
            .to_socket_addrs()?
            .next()
            .ok_or(Error::Other("Address resolution failed"))?;

        Ok(Self {
            address,
            connection: None,
            logged_in: false,
        })
    }

    pub fn connect(&mut self) -> Result<()> {
        self.connection = Some(BcConnection::new(self.address, RX_TIMEOUT)?);
        Ok(())
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
                client_idx: 0,
                encrypted: true,
                class: 0x6514,
            },
            body: BcBody::LegacyMsg(LegacyMsg::LoginMsg {
                username: md5_username,
                password: md5_password,
            }),
        };

        sub_login.send(legacy_login)?;

        let legacy_reply = sub_login.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;

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
                client_idx: 0, // TODO
                encrypted: true,
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
        let modern_reply = sub_login.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;

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

        Ok(device_info)
    }

    pub fn logout(&mut self) -> Result<()> {
        if self.logged_in {
            // TODO: Send message ID 2
        }
        self.logged_in = false;
        Ok(())
    }

    pub fn ping(&self) -> Result<()> {
        let connection = self.connection.as_ref().expect("Must be connected to ping");
        let sub_ping = connection.subscribe(MSG_ID_PING)?;

        let ping = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PING,
                client_idx: 0,
                encrypted: true,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                ..Default::default()
            }),
        };

        sub_ping.send(ping)?;

        sub_ping.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;

        Ok(())
    }

    pub fn start_video(
        &self,
        data_out: &mut dyn Write,
        stream_name: &str,
        channel_id: u32,
    ) -> Result<Never> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to start video");

        // Video
        let sub_video = connection.subscribe(MSG_ID_VIDEO)?;

        let start_video = Bc::new_from_xml(
            BcMeta {
                msg_id: MSG_ID_VIDEO,
                client_idx: 0, // TODO
                encrypted: true,
                class: 0x6414, // IDK why
            },
            BcXml {
                preview: Some(Preview {
                    version: xml_ver(),
                    channel_id,
                    handle: 0,
                    stream_type: stream_name.to_string(),
                }),
                ..Default::default()
            },
        );

        sub_video.send(start_video)?;

        let mut media_sub = MediaDataSubscriber::from_bc_sub(&sub_video);

        loop {
            let binary_data = media_sub.next_media_packet(RX_TIMEOUT)?;
            // We now have a complete interesting packet. Send it to gst.
            // Process the packet
            match binary_data.kind() {
                MediaDataKind::VideoDataIframe | MediaDataKind::VideoDataPframe => {
                    data_out.write_all(binary_data.body())?;
                }
                _ => {}
            };
        }
    }

    fn ability_support_query_user(&self, for_user: &str) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 58;
        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_ext(
            BcMeta {
                msg_id: MSG_ID,
                client_idx: 0, // TODO
                encrypted: true,
                class: 0x6414, // IDK why
            },
            Extension {
                version: xml_ver(),
                user_name: Some(for_user.to_string()),
                ..Default::default()
            },
        );
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn stream_info_query(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 146;
        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn ability_info_query_user(&self, for_user: &str) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 151;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_ext(
            BcMeta {
                msg_id: MSG_ID,
                client_idx: 0, // TODO
                encrypted: true,
                class: 0x6414, // IDK why
            },
            Extension {
                version: xml_ver(),
                user_name: Some(for_user.to_string()),
                token: Some("system, network, alarm, record, video, image".to_string()),
                ..Default::default()
            },
        );
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn unknown_192(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 192;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn start_motion_query(&self) -> Result<Bc> {
        // This message tells the camera to send the motion events to us
        // Which are the recieved on msgid 33
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 31;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn rf_alarm_query(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 133;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn hdd_query(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 102;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn version_query(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 80;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn uid_query(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 144;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn datetime_query(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 104;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn ability_info_query_camera(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 199;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn signal_query(&self) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 115;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_meta(BcMeta {
            msg_id: MSG_ID,
            client_idx: 0, // TODO
            encrypted: true,
            class: 0x6414, // IDK why
        });
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn ptz_preset_query(&self, channel_id: u32) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 190;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_ext(
            BcMeta {
                msg_id: MSG_ID,
                client_idx: 0, // TODO
                encrypted: true,
                class: 0x6414, // IDK why
            },
            Extension {
                version: xml_ver(),
                channel_id: Some(channel_id),
                ..Default::default()
            },
        );
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn audio_back_query(&self, channel_id: u32) -> Result<Bc> {
        // Currently unused in neolink other then to match the offical clients
        // login sequence during testing
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 10;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_ext(
            BcMeta {
                msg_id: MSG_ID,
                client_idx: 0, // TODO
                encrypted: true,
                class: 0x6414, // IDK why
            },
            Extension {
                version: xml_ver(),
                channel_id: Some(channel_id),
                ..Default::default()
            },
        );
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(Default::default());
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
    }

    fn full_login_sequence(&self, channel_id: u32, for_user: &str) -> Result<()> {
        // Simulates the full offical client post login sequence
        // We ignore the results, just send the queries
        // and hope for the best.
        self.ability_support_query_user(for_user)?;
        self.stream_info_query()?;
        self.unknown_192()?;
        self.start_motion_query()?;
        self.rf_alarm_query()?;
        self.hdd_query()?;
        self.version_query()?;
        self.uid_query()?;
        self.ability_info_query_user(for_user)?;
        self.datetime_query()?;
        self.ability_info_query_camera()?;
        self.signal_query()?;
        self.ptz_preset_query(channel_id)?;
        self.audio_back_query(channel_id)?;

        Ok(())
    }

    pub fn start_motion(
        &self,
        data_out: &Sender<MotionStatus>,
        channel_id: u32,
        for_user: &str,
    ) -> Result<Never> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        self.start_motion_query()?;

        let sub_motion = connection.subscribe(MSG_ID_MOTION)?;

        let motiondata_sub = MotionDataSubscriber::from_bc_sub(&sub_motion, channel_id);

        loop {
            let status = motiondata_sub.get_motion_status()?;
            if data_out.send(status).is_err() {
                error!("Failed to send motion status to reciever")
            }
        }
    }

    pub fn enable_led(&self, channel_id: u32, enable: bool) -> Result<Bc> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        const MSG_ID: u32 = 209;

        let query_sub = connection.subscribe(MSG_ID)?;
        let mut query_in = Bc::new_from_ext(
            BcMeta {
                msg_id: MSG_ID,
                client_idx: 0, // TODO
                encrypted: true,
                class: 0x6414, // IDK why
            },
            Extension {
                version: xml_ver(),
                channel_id: Some(channel_id),
                ..Default::default()
            },
        );
        if let BcBody::ModernMsg(mmsg) = &mut query_in.body {
            mmsg.payload = Some(BcPayloads::BcXml(BcXml {
                led_state: Some(LedState {
                    channel_id,
                    state: "auto".to_string(), // Infra red is either auto/close TODO: Poll current status
                    light_state: if enable {
                        "open".to_string()
                    } else {
                        "close".to_string()
                    },
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
        query_sub.send(query_in)?;

        let response = query_sub.rx.recv_timeout(RX_TIMEOUT).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => Error::TimeoutTimeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => Error::TimeoutDropped,
        })?;
        Ok(response)
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

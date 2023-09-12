use crate::bc;
use futures::stream::StreamExt;
use log::*;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, AtomicU16, Ordering},
};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use Md5Trunc::*;

mod abilityinfo;
mod battery;
mod connection;
mod credentials;
mod errors;
mod floodlight_status;
mod keepalive;
mod ledstate;
mod link;
mod login;
mod logout;
mod motion;
mod ping;
mod pirstate;
mod ptz;
mod reboot;
mod resolution;
mod snap;
mod stream;
mod stream_info;
mod talk;
mod time;
mod version;

pub(crate) use connection::*;
pub use credentials::*;
pub use errors::Error;
pub use ledstate::LightState;
pub use login::MaxEncryption;
pub use motion::{MotionData, MotionStatus};
pub use pirstate::PirState;
pub use ptz::Direction;
pub use resolution::*;
use std::sync::Arc;
pub use stream::{StreamData, StreamKind};

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy)]
enum ReadKind {
    ReadOnly,
    ReadWrite,
    None,
}

///
/// This is the primary struct of this library when interacting with the camera
///
pub struct BcCamera {
    channel_id: u8,
    connection: Arc<BcConnection>,
    logged_in: AtomicBool,
    message_num: AtomicU16,
    // Certain commands such as logout require the username/pass in plain text.... why....???
    credentials: Credentials,
    abilities: RwLock<HashMap<String, ReadKind>>,
    #[allow(dead_code)]
    cancel: CancellationToken,
}

/// Options used to construct a camera
#[derive(Debug)]
pub struct BcCameraOpt {
    /// Name, mostly used for message logs
    pub name: String,
    /// Channel the camera is on 0 unless using a NVR
    pub channel_id: u8,
    /// IPs of the camera
    pub addrs: Vec<IpAddr>,
    /// The UID of the camera
    pub uid: Option<String>,
    /// Port to try optional. When not given all known BC ports will be tried
    /// When given all known bc port AND the given port will be tried
    pub port: Option<u16>,
    /// Protocol decides if UDP/TCP are used for the camera
    pub protocol: ConnectionProtocol,
    /// Discovery method to allow
    pub discovery: DiscoveryMethods,
    /// Maximum number of retries for discovery
    pub max_discovery_retries: usize,
    /// Credentials for login
    pub credentials: Credentials,
    /// Toggle debug print of underlying data
    pub debug: bool,
}

/// Used to choose the print format of various status messages like battery levels
///
/// Currently this is just the format of battery levels but if we ever got more status
/// messages then they will also use this information
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PrintFormat {
    /// None, don't print
    None,
    /// A human readable output
    Human,
    /// Xml formatted
    Xml,
}

/// Type of connection to try
#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConnectionProtocol {
    /// TCP and UDP
    #[default]
    TcpUdp,
    /// TCP only
    Tcp,
    /// Udp only
    Udp,
}

enum CameraLocation {
    Tcp(SocketAddr),
    Udp(DiscoveryResult),
}

impl BcCamera {
    /// Try to connect to the camera via appropaite methods and return
    /// the location that should be used
    async fn find_camera(options: &BcCameraOpt) -> Result<CameraLocation> {
        let discovery = Discovery::new().await?;
        if let ConnectionProtocol::Tcp | ConnectionProtocol::TcpUdp = options.protocol {
            let mut sockets = vec![];
            match options.port {
                Some(9000) | None => {
                    for addr in options.addrs.iter() {
                        sockets.push(SocketAddr::new(*addr, 9000));
                    }
                }
                Some(n) => {
                    for addr in options.addrs.iter() {
                        sockets.push(SocketAddr::new(*addr, n));
                        sockets.push(SocketAddr::new(*addr, 9000));
                    }
                }
            }
            if !sockets.is_empty() {
                info!("{}: Trying TCP discovery", options.name);
                for socket in sockets.drain(..) {
                    let channel_id: u8 = options.channel_id;
                    if let Ok(addr) = discovery.check_tcp(socket, channel_id).await.map(|_| {
                        info!("{}: TCP Discovery success at {:?}", options.name, &socket);
                        socket
                    }) {
                        return Ok(CameraLocation::Tcp(addr));
                    }
                }
            }
        }

        if let (Some(uid), ConnectionProtocol::Udp | ConnectionProtocol::TcpUdp) =
            (options.uid.as_ref(), options.protocol)
        {
            let mut sockets = vec![];
            match options.port {
                None | Some(2015) | Some(2018) => {
                    for addr in options.addrs.iter() {
                        sockets.push(SocketAddr::new(*addr, 2018));
                        sockets.push(SocketAddr::new(*addr, 2015));
                    }
                }
                Some(n) => {
                    for addr in options.addrs.iter() {
                        sockets.push(SocketAddr::new(*addr, n));
                        sockets.push(SocketAddr::new(*addr, 2015));
                        sockets.push(SocketAddr::new(*addr, 2018));
                    }
                }
            }
            let (allow_local, allow_remote, allow_map, allow_relay) = match options.discovery {
                DiscoveryMethods::None => (false, false, false, false),
                DiscoveryMethods::Local => (true, false, false, false),
                DiscoveryMethods::Remote => (true, true, false, false),
                DiscoveryMethods::Map => (true, true, true, false),
                DiscoveryMethods::Relay => (true, true, true, true),
                DiscoveryMethods::Cellular => (false, false, true, true),
                DiscoveryMethods::Debug => (false, false, false, true),
            };

            let res = tokio::select! {
                Ok(v) = async {
                    let uid_local = uid.clone();
                    info!("{}: Trying local discovery", options.name);
                    let result = discovery.local(&uid_local, Some(sockets)).await;
                    match result {
                        Ok(disc) => {
                            info!(
                                "{}: Local discovery success {} at {}",
                                options.name,
                                uid_local,
                                disc.get_addr()
                            );
                            Ok(CameraLocation::Udp(disc))
                        },
                        Err(e) => Err(e)
                    }
                }, if allow_local => Ok(v),
                Ok(v) = async {
                    let mut discovery = Discovery::new().await?;
                    let reg_result;
                    // Registration is looped as it seems that reolink
                    // only updates the registration lazily when someone attempts
                    // to connect. The first few connects fails until the server data
                    // is updated
                    //
                    // We loop infinitly and allow the caller to timeout at the
                    // interval they desire
                    let mut retry = 0;
                    let max_retry: usize = options.max_discovery_retries;
                    loop {
                        tokio::task::yield_now().await;
                        if let Ok(result) = discovery.get_registration(uid).await {
                            reg_result = result;
                            break;
                        }
                        if retry >= max_retry && max_retry > 0 {
                            return Err(Error::DiscoveryTimeout);
                        }
                        log::info!("{}: Registration with reolink servers failed. Retrying: {}/{}", options.name, retry + 1, if max_retry > 0 {format!("{}", max_retry)} else {"infinite".to_string()});
                        retry += 1;
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        // New discovery to get new client IDs
                        discovery = Discovery::new().await?;
                    };
                    tokio::select! {
                        Ok(v) = async {
                            let uid_remote = uid.clone();
                            info!("{}: Trying remote discovery", options.name);
                            let result = discovery
                                .remote(&uid_remote, &reg_result)
                                .await;
                            match result {
                                Ok(disc) => {
                                    info!(
                                        "{}: Remote discovery success {} at {}",
                                        options.name,
                                        uid_remote,
                                        disc.get_addr()
                                    );
                                    Ok(CameraLocation::Udp(disc))
                                },
                                Err(e) => Err(e)
                            }
                        }, if allow_remote => Ok(v),
                        Ok(v) = async {
                            let uid_map = uid.clone();
                            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
                            info!("{}: Trying map discovery", options.name);
                            let result = discovery.map(&reg_result).await;
                            match result {
                                Ok(disc) => {
                                    info!(
                                        "{}: Map success {} at {}",
                                        options.name,
                                        uid_map,
                                        disc.get_addr()
                                    );
                                    Ok(CameraLocation::Udp(disc))
                                },
                                Err(e) => Err(e),
                            }
                        }, if allow_map => Ok(v),
                        Ok(v) = async {
                            let uid_relay = uid.clone();
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                            info!("{}: Trying relay discovery", options.name);
                            let result = discovery.relay(&reg_result).await;
                            match result {
                                Ok(disc) => {
                                    info!(
                                        "{}: Relay success {} at {}",
                                        options.name,
                                        uid_relay,
                                        disc.get_addr()
                                    );
                                    Ok(CameraLocation::Udp(disc))
                                },
                                Err(e) => Err(e),
                            }
                        }, if allow_relay => Ok(v),
                        else => Err(Error::DiscoveryTimeout),
                    }
                }, if allow_remote || allow_map || allow_relay => Ok(v),
                else => Err(Error::DiscoveryTimeout),
            }?;

            return Ok(res);
        }

        info!("{}: Discovery failed", options.name);
        // Nothing works
        Err(Error::CannotInitCamera)
    }

    ///
    /// Create a new camera interface
    ///
    /// # Parameters
    ///
    /// * `options` - Camera information see [`BcCameraOpt]
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new(options: &BcCameraOpt) -> Result<Self> {
        let username: String = options.credentials.username.clone();
        let passwd: Option<String> = options.credentials.password.clone();

        let (sink, source): (BcConnSink, BcConnSource) = {
            match BcCamera::find_camera(options).await? {
                CameraLocation::Tcp(addr) => {
                    let (x, r) = TcpSource::new(addr, &username, passwd.as_ref(), options.debug)
                        .await?
                        .split();
                    (Box::new(x), Box::new(r))
                }
                CameraLocation::Udp(discovery) => {
                    let (x, r) = UdpSource::new_from_discovery(
                        discovery,
                        &username,
                        passwd.as_ref(),
                        options.debug,
                    )
                    .await?
                    .split();
                    (Box::new(x), Box::new(r))
                }
            }
        };

        let conn = BcConnection::new(sink, source).await?;

        trace!("Success");
        let me = Self {
            connection: Arc::new(conn),
            message_num: AtomicU16::new(0),
            channel_id: options.channel_id,
            logged_in: AtomicBool::new(false),
            credentials: Credentials::new(username, passwd),
            abilities: Default::default(),
            cancel: CancellationToken::new(),
        };
        me.keepalive().await?;
        Ok(me)
    }

    /// This method will get a new message number and increment the message count atomically
    pub fn new_message_num(&self) -> u16 {
        self.message_num.fetch_add(1, Ordering::Relaxed)
    }

    fn get_connection(&self) -> Arc<BcConnection> {
        self.connection.clone()
    }

    // Certains commands like logout need the username and password
    // this command will return
    // This will only work after login
    fn get_credentials(&self) -> &Credentials {
        &self.credentials
    }

    async fn has_ability<T: Into<String>>(&self, name: T) -> ReadKind {
        let abilities = self.abilities.read().await;
        if let Some(kind) = abilities.get(&name.into()).copied() {
            kind
        } else {
            ReadKind::None
        }
    }
    async fn has_ability_ro<T: Into<String>>(&self, name: T) -> Result<()> {
        let s: String = name.into();
        match self.has_ability(&s).await {
            ReadKind::ReadWrite | ReadKind::ReadOnly => Ok(()),
            ReadKind::None => Err(Error::MissingAbility {
                name: s.clone(),
                requested: "read".to_string(),
                actual: "none".to_string(),
            }),
        }
    }
    async fn has_ability_rw<T: Into<String>>(&self, name: T) -> Result<()> {
        let s: String = name.into();
        match self.has_ability(&s).await {
            ReadKind::ReadWrite => Ok(()),
            ReadKind::ReadOnly => Err(Error::MissingAbility {
                name: s.clone(),
                requested: "write".to_string(),
                actual: "read".to_string(),
            }),
            ReadKind::None => Err(Error::MissingAbility {
                name: s.clone(),
                requested: "write".to_string(),
                actual: "none".to_string(),
            }),
        }
    }

    /// Wait for all thread to finish
    ///
    /// If an error is returned in any thread it will return the first error
    pub async fn join(&self) -> Result<()> {
        self.connection.join().await
    }

    /// Disconnect from the camera. This is done by sending cancel to
    /// all threads then waiting for the join
    pub async fn shutdown(&self) -> Result<()> {
        self.connection.shutdown().await?;
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

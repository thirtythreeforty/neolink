use crate::bc;
use log::*;
use std::convert::TryInto;
use std::net::ToSocketAddrs;
use std::sync::{
    atomic::{AtomicBool, AtomicU16, Ordering},
    Mutex,
};

use Md5Trunc::*;

mod connection;
mod errors;
mod ledstate;
mod login;
mod logout;
mod motion;
mod ping;
mod pirstate;
mod ptz;
mod reboot;
mod resolution;
mod stream;
mod talk;
mod time;
mod version;

pub(crate) use connection::*;
pub use errors::Error;
pub use ledstate::LightState;
pub use motion::{MotionData, MotionStatus};
pub use pirstate::PirState;
pub use ptz::Direction;
pub use resolution::*;
use std::sync::Arc;
pub use stream::{Stream, StreamData};

pub(crate) type Result<T> = std::result::Result<T, Error>;

impl From<crossbeam_channel::RecvTimeoutError> for Error {
    fn from(k: crossbeam_channel::RecvTimeoutError) -> Self {
        match k {
            crossbeam_channel::RecvTimeoutError::Timeout => Error::Timeout,
            crossbeam_channel::RecvTimeoutError::Disconnected => Error::TimeoutDisconnected,
        }
    }
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
    credentials: Mutex<Option<Credentials>>,
}

// Used for caching the credentials
#[derive(Clone)]
struct Credentials {
    username: String,
    password: Option<String>,
}

impl Credentials {
    fn new(username: String, password: Option<String>) -> Self {
        Self { username, password }
    }
}

impl Drop for BcCamera {
    fn drop(&mut self) {
        debug!("Dropping camera");
        self.disconnect();
    }
}

impl BcCamera {
    ///
    /// Create a new camera interface with this address and channel ID
    ///
    /// # Parameters
    ///
    /// * `host` - The address of the camera either ip address or hostname string
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new_with_addr<T: ToSocketAddrs>(host: T, channel_id: u8) -> Result<Self> {
        let addr_iter = match host.to_socket_addrs() {
            Ok(iter) => iter,
            Err(_) => return Err(Error::AddrResolutionError),
        };
        for addr in addr_iter {
            if let Ok(cam) = Self::new(SocketAddrOrUid::SocketAddr(addr), channel_id).await {
                return Ok(cam);
            }
        }

        Err(Error::Timeout)
    }

    ///
    /// Create a new camera interface with this uid and channel ID
    ///
    /// # Parameters
    ///
    /// * `uid` - The uid of the camera
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new_with_uid(uid: &str, channel_id: u8) -> Result<Self> {
        Self::new(SocketAddrOrUid::Uid(uid.to_string()), channel_id).await
    }

    ///
    /// Create a new camera interface with this address/uid and channel ID
    ///
    /// This method will first perform hostname resolution on the address
    /// then fallback to uid if that resolution fails.
    ///
    /// Be aware that it is possible (although unlikely) that there is
    /// a dns entry with the same address as the uid. If uncertain use
    /// one of the other methods.
    ///
    /// # Parameters
    ///
    /// * `host` - The address of the camera either ip address, hostname string, or uid
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new_with_addr_or_uid<T: ToSocketAddrsOrUid>(
        host: T,
        channel_id: u8,
    ) -> Result<Self> {
        let addr_iter = match host.to_socket_addrs_or_uid() {
            Ok(iter) => iter,
            Err(_) => return Err(Error::AddrResolutionError),
        };
        for addr_or_uid in addr_iter {
            if let Ok(cam) = Self::new(addr_or_uid, channel_id).await {
                return Ok(cam);
            }
        }

        Err(Error::Timeout)
    }

    ///
    /// Create a new camera interface with this address/uid and channel ID
    ///
    /// # Parameters
    ///
    /// * `addr` - An enum of [`SocketAddrOrUid`] that contains the address
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new(addr: SocketAddrOrUid, channel_id: u8) -> Result<Self> {
        let source: Box<dyn Source> = match addr {
            SocketAddrOrUid::SocketAddr(addr) => {
                debug!("Trying address {}", addr);
                Box::new(TcpSource::new(addr).await?)
            }
            SocketAddrOrUid::Uid(uid) => {
                debug!("Trying uid {}", uid);
                let discovery = Discovery::local(&uid).await?;
                Box::new(UdpSource::new_from_discovery(discovery).await?)
            }
        };

        let conn = BcConnection::new(source).await?;

        debug!("Success");
        let me = Self {
            connection: Arc::new(conn),
            message_num: AtomicU16::new(0),
            channel_id,
            logged_in: AtomicBool::new(false),
            credentials: Mutex::new(None),
        };
        Ok(me)
    }

    /// This method will get a new message number and increment the message count atomically
    pub fn new_message_num(&self) -> u16 {
        self.message_num.fetch_add(1, Ordering::Relaxed)
    }

    fn get_connection(&self) -> Arc<BcConnection> {
        self.connection.clone()
    }

    /// This will drop the connection.
    pub fn disconnect(&mut self) {
        // Stop polling now. We don't need it for a disconnect
        //
        // It will also ensure that when we drop the connection we don't
        // get an error for read return zero bytes from the polling thread;
        self.connection.stop_polling();
    }

    // Certains commands like logout need the username and password
    // this command will return it as a tuple of (Username, Option<Password>)
    // This will only work after login
    fn get_credentials(&self) -> Option<Credentials> {
        self.credentials.lock().unwrap().clone()
    }
    // This is used to store the credentials it is called during login.
    fn set_credentials(&self, username: String, password: Option<String>) {
        *(self.credentials.lock().unwrap()) = Some(Credentials::new(username, password));
    }
    // This is used to clear the stored credentials it is called during logout.
    fn clear_credentials(&self) {
        *(self.credentials.lock().unwrap()) = None;
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

/// This is a convience function to make an AES key from the login password and the NONCE
/// negotiated during login
pub fn make_aes_key(nonce: &str, passwd: &str) -> [u8; 16] {
    let key_phrase = format!("{}-{}", nonce, passwd);
    let key_phrase_hash = format!("{:X}\0", md5::compute(key_phrase))
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

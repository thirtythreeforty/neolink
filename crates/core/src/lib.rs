#![warn(missing_docs)]
//! # Neolink-Core
//!
//! Neolink-Core is a rust library for interacting with reolink and family cameras.
//!
//! Most high level camera controls are in the [`bc_protocol`] module
//!
//! A camera can be initialised with
//!
//! ```no_run
//! use neolink_core::bc_protocol::BcCamera;
//! let channel_id = 0; // Usually zero but can be non zero if uses a reolink NVR
//! let mut camera = BcCamera::new_with_addr("camera_ip_address", channel_id).unwrap();
//! ```
//!
//! After that login can be conducted with
//!
//! ```no_run
//! # use neolink_core::bc_protocol::BcCamera;
//! # let channel_id = 0;
//! # let mut camera = BcCamera::new_with_addr("camera_ip_address", channel_id).unwrap();
//! camera.login("username", Some("password"));
//! ```
//! For further commands see the [`bc_protocol::BcCamera`] struct.
//!

/// Contains low level BC structures and formats
pub mod bc;
/// Contains high level interfaces for the camera
pub mod bc_protocol;
/// Contains low level structures and formats for the media substream
pub mod bcmedia;
///  Contains low level structures and formats for the udpstream
pub mod bcudp;

/// This is the top level error structure of the library
///
/// Most commands will either return their `Ok(result)` or this `Err(Error)`
pub use bc_protocol::Error;

pub(crate) use bc_protocol::Result;

pub(crate) type NomErrorType<'a> = nom::error::VerboseError<&'a [u8]>;

// Used for caching the credentials
#[derive(Clone)]
pub(crate) struct Credentials {
    username: String,
    password: Option<String>,
}

impl Default for Credentials {
    /// Default credentials for some reolink cameras
    fn default() -> Self {
        Self {
            username: "admin".to_string(),
            password: Some("123456".to_string()),
        }
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entry(&"username", &self.username)
            .entry(&"password", &"******")
            .finish()
    }
}

use std::convert::TryInto;
impl Credentials {
    pub(crate) fn new<T: Into<String>, U: Into<String>>(username: T, password: Option<U>) -> Self {
        Self {
            username: username.into(),
            password: password.map(|t| t.into()),
        }
    }

    /// This is a convience function to make an AES key from the login password and the NONCE
    /// negotiated during login
    pub(crate) fn make_aeskey<T: AsRef<str>>(&self, nonce: T) -> [u8; 16] {
        let key_phrase = format!(
            "{}-{}",
            nonce.as_ref(),
            self.password.clone().unwrap_or_default()
        );
        let key_phrase_hash = format!("{:X}\0", md5::compute(key_phrase))
            .to_uppercase()
            .into_bytes();
        key_phrase_hash[0..16].try_into().unwrap()
    }
}

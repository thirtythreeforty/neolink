//! Handles credentials for camera including default reolink password

use std::convert::TryInto;

/// Used for caching and supplying the credentials
#[derive(Clone)]
pub struct Credentials {
    /// The username to login to the camera with
    pub username: String,
    /// The password to use for login. Some camera allow this to be ommited
    pub password: Option<String>,
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

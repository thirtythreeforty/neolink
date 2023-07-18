// Data for the basic connected state
//
// This state has formed the TCP/UDP tunnel
// but has not logged in
use super::{camera::Camera, disconnected::Disconnected, loggedin::LoggedIn};
use crate::utils::timeout;
use anyhow::Result;

use neolink_core::bc_protocol::{BcCamera, MaxEncryption};

pub(crate) struct Connected {
    pub(crate) camera: BcCamera,
}

impl Camera<Connected> {
    #[allow(dead_code)]
    pub(crate) async fn disconnect(self) -> Result<Camera<Disconnected>> {
        Ok(Camera {
            shared: self.shared,
            state: Disconnected {},
        })
    }

    pub(crate) async fn login(self) -> Result<Camera<LoggedIn>> {
        let max_encryption = match self.shared.config.max_encryption.to_lowercase().as_str() {
            "none" => MaxEncryption::None,
            "bcencrypt" => MaxEncryption::BcEncrypt,
            "aes" => MaxEncryption::Aes,
            _ => MaxEncryption::Aes,
        };

        timeout(self.state.camera.login_with_maxenc(max_encryption)).await??;
        Ok(Camera {
            shared: self.shared,
            state: LoggedIn {
                camera: self.state.camera,
            },
        })
    }

    #[allow(unused)]
    pub(crate) async fn join(&self) -> Result<()> {
        self.state
            .camera
            .join()
            .await
            .map_err(|e| anyhow::anyhow!("Camera join error: {:?}", e))
    }
}

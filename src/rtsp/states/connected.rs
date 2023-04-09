// Data for the basic connected state
//
// This state has formed the TCP/UDP tunnel
// but has not logged in
use super::{camera::Camera, disconnected::Disconnected, loggedin::LoggedIn};
use anyhow::Result;

use neolink_core::bc_protocol::BcCamera;

pub(crate) struct Connected {
    pub(crate) camera: BcCamera,
}

impl Camera<Connected> {
    pub(crate) async fn disconnect(self) -> Result<Camera<Disconnected>> {
        Ok(Camera {
            shared: self.shared,
            state: Disconnected {},
        })
    }

    pub(crate) async fn login(self) -> Result<Camera<LoggedIn>> {
        Ok(Camera {
            shared: self.shared,
            state: LoggedIn {
                camera: self.state.camera,
            },
        })
    }
}

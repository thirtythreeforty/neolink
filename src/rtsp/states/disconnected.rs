//! Data for the basic disconnected state
//!
//! This state has NOT formed the TCP/UDP tunnel
//! it has all the data required to connect but
//! has not estabilished the connection
//!
//! This is meant to be used to conserve battery
//! by completely disconnecting
use super::Shared;
use super::{camera::Camera, connected::Connected};
use crate::config::{CameraConfig, UserConfig};
use crate::rtsp::gst::NeoRtspServer;
use anyhow::{Context, Result};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct Disconnected {}

impl Camera<Disconnected> {
    pub(crate) async fn new(
        config: CameraConfig,
        users: &[UserConfig],
        rtsp: Arc<NeoRtspServer>,
    ) -> Result<Camera<Disconnected>> {
        Ok(Camera {
            state: Disconnected {},
            shared: Arc::new(Shared::new(config, users, rtsp).await?),
        })
    }

    pub(crate) async fn connect(self) -> Result<Camera<Connected>> {
        let camera = {
            let camera_config = &self.shared.config;
            self.shared
                .addr
                .connect_camera(camera_config)
                .await
                .with_context(|| {
                    format!(
                        "Failed to connect to camera {} at {} on channel {}",
                        camera_config.name, self.shared.addr, camera_config.channel_id
                    )
                })
        }?;

        Ok(Camera {
            shared: self.shared,
            state: Connected { camera },
        })
    }
}

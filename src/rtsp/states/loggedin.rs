// Data for the logged in state
//
// This state is logged in but is not
// streaming data
//
// It can be used to alter settings etc

use super::{camera::Camera, connected::Connected, streaming::Streaming};
use anyhow::Result;
use log::*;
use neolink_core::bc_protocol::BcCamera;

pub(crate) struct LoggedIn {
    pub(crate) camera: BcCamera,
}

impl Camera<LoggedIn> {
    #[allow(dead_code)]
    pub(crate) async fn logout(self) -> Result<Camera<Connected>> {
        self.state.camera.logout().await?;
        Ok(Camera {
            shared: self.shared,
            state: Connected {
                camera: self.state.camera,
            },
        })
    }

    pub(crate) async fn stream(self) -> Result<Camera<Streaming>> {
        Camera::<Streaming>::from_login(self).await
    }

    pub(crate) async fn manage(&self) -> Result<()> {
        let cam_time = self.state.camera.get_time().await?;
        let mut update = false;
        if let Some(time) = cam_time {
            info!(
                "{}: Camera time is already set: {}",
                self.shared.config.name, time
            );
            if self.shared.config.update_time {
                update = true;
            }
        } else {
            update = true;
            warn!(
                "{}: Camera has no time set, Updating",
                self.shared.config.name
            );
        }
        if update {
            use time::OffsetDateTime;
            let new_time =
                OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());

            info!("{}: Setting time to {}", self.shared.config.name, new_time);
            match self.state.camera.set_time(new_time).await {
                Ok(_) => {
                    let cam_time = self.state.camera.get_time().await?;
                    if let Some(time) = cam_time {
                        info!(
                            "{}: Camera time is now set: {}",
                            self.shared.config.name, time
                        );
                    }
                }
                Err(e) => {
                    error!(
                        "{}: Camera did not accept new time (is {} an admin?): Error: {:?}",
                        self.shared.config.name, self.shared.config.username, e
                    );
                }
            }
        }

        use neolink_core::bc::xml::VersionInfo;
        if let Ok(VersionInfo {
            firmwareVersion: firmware_version,
            ..
        }) = self.state.camera.version().await
        {
            info!(
                "{}: Camera reports firmware version {}",
                self.shared.config.name, firmware_version
            );
        } else {
            info!(
                "{}: Could not fetch version information",
                self.shared.config.name
            );
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn get_camera(&self) -> &BcCamera {
        &self.state.camera
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

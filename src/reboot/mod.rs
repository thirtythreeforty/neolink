///
/// # Neolink Reboot
///
/// This module handles the reboot subcommand
///
/// The subcommand attepts to reboot the camera.
///
/// # Usage
///
/// ```bash
/// neolink reboot --config=config.toml CameraName
/// ```
///
use anyhow::{anyhow, Context, Result};
use log::*;
use neolink_core::bc_protocol::BcCamera;
use std::fs;
use validator::Validate;

mod cmdline;
mod config;

pub(crate) use cmdline::Opt;
use config::Config;

/// Entry point for the reboot subcommand
///
/// Opt is the command line options
pub fn main(opt: Opt) -> Result<()> {
    let config: Config = toml::from_str(
        &fs::read_to_string(&opt.config)
            .with_context(|| format!("Failed to read {:?}", &opt.config))?,
    )
    .with_context(|| format!("Failed to load {:?} as a config file", &opt.config))?;

    config
        .validate()
        .with_context(|| format!("Failed to validate the {:?} config file", &opt.config))?;

    let mut cam_found = false;
    for camera_config in &config.cameras {
        if opt.camera == camera_config.name {
            cam_found = true;
            info!(
                "{}: Connecting to camera at {}",
                camera_config.name, camera_config.camera_addr
            );

            let mut camera =
                BcCamera::new_with_addr(&camera_config.camera_addr, camera_config.channel_id)
                    .with_context(|| {
                        format!(
                            "Failed to connect to camera {} at {} on channel {}",
                            camera_config.name, camera_config.camera_addr, camera_config.channel_id
                        )
                    })?;

            info!("{}: Logging in", camera_config.name);
            camera
                .login(&camera_config.username, camera_config.password.as_deref())
                .with_context(|| format!("Failed to login to {}", camera_config.name))?;

            info!("{}: Connected and logged in", camera_config.name);

            camera
                .reboot()
                .context("Could not send reboot command to the camera")?;
        }
    }

    if !cam_found {
        Err(anyhow!(
            "Camera {} was not in the config file {:?}",
            &opt.camera,
            &opt.config
        ))
    } else {
        Ok(())
    }
}

///
/// # Neolink Status LED
///
/// This module handles the controls of the blue led status light
///
/// The subcommand attepts to set the LED status light not the IR
/// lights or the flood lights.
///
/// # Usage
///
/// ```bash
/// # To turn the light on
/// neolink status-light --config=config.toml CameraName on
/// # Or off
/// neolink status-light --config=config.toml CameraName off
/// ```
///
use anyhow::{anyhow, Context, Result};
use log::*;
use std::fs;
use validator::Validate;

mod cmdline;
mod config;

use crate::utils::AddressOrUid;
pub(crate) use cmdline::Opt;
use config::Config;

/// Entry point for the ledstatus subcommand
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

            let camera_addr =
                AddressOrUid::new(&camera_config.camera_addr, &camera_config.camera_uid).unwrap();
            info!(
                "{}: Connecting to camera at {}",
                camera_config.name, camera_addr
            );

            let mut camera = camera_addr
                .connect_camera(camera_config.channel_id)
                .with_context(|| {
                    format!(
                        "Failed to connect to camera {} at {} on channel {}",
                        camera_config.name, camera_addr, camera_config.channel_id
                    )
                })?;

            info!("{}: Logging in", camera_config.name);
            camera
                .login(&camera_config.username, camera_config.password.as_deref())
                .with_context(|| format!("Failed to login to {}", camera_config.name))?;

            info!("{}: Connected and logged in", camera_config.name);

            camera
                .led_light_set(opt.on)
                .context("Unable to set camera light state")?;
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

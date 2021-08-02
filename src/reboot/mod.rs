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
use log::*;
use neolink_core::bc_protocol::BcCamera;
use std::fs;
use validator::Validate;

mod cmdline;
mod config;
mod errors;

pub(crate) use cmdline::Opt;
use config::Config;
pub(crate) use errors::Error;

/// Entry point for the reboot subcommand
///
/// Opt is the command line options
pub fn main(opt: Opt) -> Result<(), Error> {
    let config: Config = toml::from_str(&fs::read_to_string(opt.config)?)?;

    config.validate()?;

    let mut cam_found = false;
    for camera_config in &config.cameras {
        if opt.camera == camera_config.name {
            cam_found = true;
            info!(
                "{}: Connecting to camera at {}",
                camera_config.name, camera_config.camera_addr
            );

            let mut camera =
                BcCamera::new_with_addr(&camera_config.camera_addr, camera_config.channel_id)?;

            info!("{}: Logging in", camera_config.name);
            camera.login(&camera_config.username, camera_config.password.as_deref())?;

            info!("{}: Connected and logged in", camera_config.name);

            camera.reboot()?;
        }
    }

    if !cam_found {
        error!(
            "No camera with the name {} was found in the config",
            opt.camera
        );
    }

    Ok(())
}

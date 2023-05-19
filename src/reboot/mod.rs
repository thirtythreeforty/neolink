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
use anyhow::{Context, Result};

mod cmdline;

use super::config::Config;
use crate::utils::find_and_connect;
pub(crate) use cmdline::Opt;

/// Entry point for the reboot subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config) -> Result<()> {
    let camera = find_and_connect(&config, &opt.camera).await?;

    camera
        .reboot()
        .await
        .context("Could not send reboot command to the camera")?;
    Ok(())
}

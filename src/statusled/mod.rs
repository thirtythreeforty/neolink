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
use anyhow::{Context, Result};

mod cmdline;

use super::config::Config;
use crate::utils::find_and_connect;
pub(crate) use cmdline::Opt;

/// Entry point for the ledstatus subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config) -> Result<()> {
    let camera = find_and_connect(&config, &opt.camera).await?;

    camera
        .led_light_set(opt.on)
        .await
        .context("Unable to set camera light state")?;
    Ok(())
}

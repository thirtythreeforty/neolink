///
/// # Neolink PIR
///
/// This module handles the controls of the pir sensor alarm
///
///
/// # Usage
///
/// ```bash
/// # To turn the pir sensor on
/// neolink pir --config=config.toml CameraName on
/// # Or off
/// neolink pir --config=config.toml CameraName off
/// ```
///
use anyhow::{Context, Result};

mod cmdline;

use super::config::Config;
use crate::utils::find_and_connect;
pub(crate) use cmdline::Opt;

/// Entry point for the pir subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config) -> Result<()> {
    let camera = find_and_connect(&config, &opt.camera).await?;

    camera
        .pir_set(opt.on)
        .await
        .context("Unable to set camera PIR state")?;
    Ok(())
}

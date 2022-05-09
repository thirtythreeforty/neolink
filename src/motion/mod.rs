///
/// # Neolink Motion
///
/// This module handles the controls of the motion alarm
///
///
/// # Usage
///
/// ```bash
/// # To turn the motion on
/// neolink motion --config=config.toml CameraName on
/// # Or off
/// neolink motion --config=config.toml CameraName off
/// ```
///
use anyhow::{Context, Result};

mod cmdline;

use super::config::Config;
use crate::utils::find_and_connect;
pub(crate) use cmdline::Opt;

/// Entry point for the motion subcommand
///
/// Opt is the command line options
pub(crate) fn main(opt: Opt, config: Config) -> Result<()> {
    let mut camera = find_and_connect(&config, &opt.camera)?;

    camera
        .motion_set(opt.on)
        .context("Unable to set camera motion state")?;
    Ok(())
}

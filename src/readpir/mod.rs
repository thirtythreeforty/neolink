///
/// # Neolink PIR
///
/// This module reads the status of the pir sensor alarm
///
///
/// # Usage
///
/// ```bash
/// # To turn the pir sensor on
/// neolink readpir --config=config.toml CameraName
/// # return true or false
///
use anyhow::{Context, Result};

mod cmdline;

use super::config::Config;
use crate::utils::find_and_connect;
pub(crate) use cmdline::Opt;

/// Entry point for the pir subcommand
///
/// Opt is the command line options
pub(crate) fn main(opt: Opt, config: Config) -> Result<u8> {
    let mut camera = find_and_connect(&config, &opt.camera)?;
    let result = camera.get_pirstate()
        .context("Error retrieving camera pir state")?;
    Ok(result.enable)
}

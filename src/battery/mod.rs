///
/// # Neolink Battery
///
/// This module handles the printing of the Battery status
/// in xml format
///
/// # Usage
///
/// ```bash
/// neolink battery --config=config.toml CameraName
/// ```
///
use anyhow::{Context, Result};

mod cmdline;

use super::config::Config;
use crate::utils::find_and_connect;
pub(crate) use cmdline::Opt;

/// Entry point for the battery subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config) -> Result<()> {
    let camera = find_and_connect(&config, &opt.camera).await?;

    let state = camera
        .battery_info()
        .await
        .context("Unable to get camera Battery state")?;
    let ser = String::from_utf8(
        yaserde::ser::serialize_with_writer(&state, vec![], &Default::default())
            .expect("Should Ser the struct"),
    )
    .expect("Should be UTF8");
    println!("{}", ser);

    let _ = camera.logout().await;
    camera.shutdown().await?;

    Ok(())
}

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
use crate::common::neocam::NeoReactor;
pub(crate) use cmdline::Opt;

/// Entry point for the reboot subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config, reactor: NeoReactor) -> Result<()> {
    let config = config.get_camera_config(&opt.camera)?;
    let camera = reactor.get_or_insert(config.clone()).await?;

    camera
        .run_task(|camera| {
            Box::pin(async move {
                camera
                    .reboot()
                    .await
                    .context("Could not send reboot command to the camera")
            })
        })
        .await?;

    camera.shutdown().await;

    Ok(())
}

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

use crate::common::NeoReactor;
pub(crate) use cmdline::Opt;

/// Entry point for the reboot subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, reactor: NeoReactor) -> Result<()> {
    let camera = reactor.get(&opt.camera).await?;

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

    Ok(())
}

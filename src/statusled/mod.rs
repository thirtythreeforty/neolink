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
use crate::common::neocam::NeoReactor;
pub(crate) use cmdline::Opt;

/// Entry point for the ledstatus subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config, reactor: NeoReactor) -> Result<()> {
    let config = config.get_camera_config(&opt.camera)?;
    let camera = reactor.get_or_insert(config.clone()).await?;

    let on = opt.on;
    camera
        .run_task(|camera| {
            Box::pin(async move {
                camera
                    .led_light_set(on)
                    .await
                    .context("Unable to set camera light state")
            })
        })
        .await?;

    camera.shutdown().await;

    Ok(())
}

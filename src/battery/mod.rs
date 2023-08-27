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

use crate::common::neocam::NeoReactor;

use super::config::Config;
pub(crate) use cmdline::Opt;

/// Entry point for the battery subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config, reactor: NeoReactor) -> Result<()> {
    let config = config.get_camera_config(&opt.camera)?;
    let camera = reactor.get_or_insert(config.clone()).await?;
    log::debug!("Battery: Instance aquired");

    let state = camera
        .run_task(|cam| {
            Box::pin(async move {
                cam.battery_info()
                    .await
                    .context("Unable to get camera Battery state")
            })
        })
        .await?;

    let ser = String::from_utf8(
        yaserde::ser::serialize_with_writer(&state, vec![], &Default::default())
            .expect("Should Ser the struct"),
    )
    .expect("Should be UTF8");
    println!("{}", ser);

    camera.shutdown().await;

    Ok(())
}

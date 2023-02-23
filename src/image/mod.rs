///
/// # Neolink Image
///
/// This module can be used to send dump a still image from the camera
///
///
/// # Usage
/// ```bash
/// neolink image --config=config.toml --finename=filepath CameraName
/// ```
///
use anyhow::{Context, Result};
use neolink_core::{bc_protocol::*, bcmedia::model::*};

mod cmdline;
mod gst;

use super::config::{CameraConfig, Config};
use crate::utils::{connect_and_login, find_camera_by_name};
pub(crate) use cmdline::Opt;

/// Entry point for the image subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config) -> Result<()> {
    let camera_config = find_camera_by_name(&config, &opt.camera).context(
        "failed to load config file or find the camera. Check the path and name of the camera.",
    )?;

    let (vid_type, buf) = fetch_iframe(camera_config).await?;
    let sender = gst::from_input(vid_type, &opt.file_path).await?;
    sender.send(buf).await?;
    // So I tried 1 iFrame and it wouldn't finish the convert (just seems to hang waiting for data)
    // I tried sending the SAME iFrame twice and SOMETIMES it converts (othertimes it just hangs)
    // Then tried two different iFrames and it always converted. Therefore we do that
    let (_, buf) = fetch_iframe(camera_config).await?;
    sender.send(buf).await?;
    sender.eos().await?;

    Ok(())
}

async fn fetch_iframe(camera_config: &CameraConfig) -> Result<(VideoType, Vec<u8>)> {
    let camera = connect_and_login(camera_config)
        .await
        .context("Failed to connect to the camera, check credentials and network")?;
    let mut stream_data = camera
        .start_video(StreamKind::Main, 0)
        .await
        .context("Failed to start video")?;

    loop {
        log::info!("Awaiting data");
        let data = stream_data.get_data().await??;
        log::info!("Got data");
        if let BcMedia::Iframe(iframedata) = data {
            return Ok((iframedata.video_type, iframedata.data));
        } else {
            log::info!("Got non iframe");
        }
    }
}

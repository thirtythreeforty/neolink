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
use log::*;
use neolink_core::{bc_protocol::*, bcmedia::model::*};

mod cmdline;
mod gst;

use super::config::Config;
use crate::utils::{connect_and_login, find_camera_by_name};
pub(crate) use cmdline::Opt;

/// Entry point for the image subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config) -> Result<()> {
    let camera_config = find_camera_by_name(&config, &opt.camera).context(
        "failed to load config file or find the camera. Check the path and name of the camera.",
    )?;

    // Set up camera to recieve the stream
    let camera = connect_and_login(camera_config)
        .await
        .context("Failed to connect to the camera, check credentials and network")?;
    let mut stream_data = camera
        .start_video(StreamKind::Main, 0)
        .await
        .context("Failed to start video")?;

    // Get one iframe as the start while also getting the getting the video type
    let buf;
    let vid_type;
    loop {
        if let BcMedia::Iframe(frame) = stream_data.get_data().await?? {
            vid_type = frame.video_type;
            buf = frame.data;
            break;
        }
    }

    let mut sender = gst::from_input(vid_type, &opt.file_path).await?;
    sender.send(buf).await?; // Send first iframe

    // Keep sending both IFrame or PFrame until finished
    while sender.is_finished().await.is_none() {
        let buf = match stream_data.get_data().await?? {
            BcMedia::Iframe(frame) => frame.data,
            BcMedia::Pframe(frame) => frame.data,
            _ => {
                continue;
            }
        };

        debug!("Sending frame data to gstreamer");
        if sender.send(buf).await.is_err() {
            // Assume that the sender is closed
            // because the pipeline is finished
            break;
        }
    }
    debug!("Sending EOS");
    let _ = sender.eos().await; // Ignore return because if pipeline is finished this will error

    Ok(())
}

///
/// # Neolink Image
///
/// This module can be used to dump a still image from the camera
///
///
/// # Usage
/// ```bash
/// neolink image --config=config.toml --file-path=filepath CameraName
/// ```
///
/// Cameras that do not support the SNAP command need to use `--use_stream`
/// which will make the camera play the stream and transcode the video into a jpeg
/// e.g.:
///
/// ```bash
/// neolink image --config=config.toml --use_stream --file-path=filepath CameraName
/// ```
///
use anyhow::{Context, Result};
use log::*;
use neolink_core::{bc_protocol::*, bcmedia::model::*};
use tokio::{fs::File, io::AsyncWriteExt};

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

    if opt.use_stream {
        let mut stream_data = camera
            .start_video(StreamKind::Main, 0, camera_config.strict)
            .await
            .context("Failed to start video")?;

        // Get one iframe at the start while also getting the the video type
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
    } else {
        // Simply use the snap command
        debug!("Using the snap command");
        let file_path = opt.file_path.with_extension("jpeg");
        let mut buffer = File::create(file_path).await?;
        let jpeg_data = camera.get_snapshot().await?;
        buffer.write_all(jpeg_data.as_slice()).await?;
    }

    Ok(())
}

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
use futures::stream::StreamExt;
use log::*;
use neolink_core::bc_protocol::*;
use tokio::{fs::File, io::AsyncWriteExt};
use tokio_stream::wrappers::BroadcastStream;

mod cmdline;
mod gst;

use crate::common::{NeoReactor, StampedData, VidFormat};
pub(crate) use cmdline::Opt;

/// Entry point for the image subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, reactor: NeoReactor) -> Result<()> {
    let camera = reactor.get(&opt.camera).await?;

    if opt.use_stream {
        let stream_data = camera
            .stream(StreamKind::Main)
            .await
            .context("Failed to start video")?;

        // Get one iframe at the start while also getting the the video type
        let mut stream_config = stream_data.config.clone();
        let vid_type = stream_config
            .wait_for(|config| config.vid_format != VidFormat::None)
            .await?
            .vid_format
            .clone();
        let mut stream = BroadcastStream::new(stream_data.vid.resubscribe())
            .filter(|f| futures::future::ready(f.is_ok())); // Filter to ignore lagged
        let buf = loop {
            if let Some(Ok(StampedData {
                data,
                ts: _,
                keyframe,
            })) = stream.next().await
            {
                if keyframe {
                    break data;
                }
            }
        };

        let mut sender = gst::from_input(vid_type, &opt.file_path).await?;
        sender.send(buf).await?; // Send first iframe

        // Keep sending both IFrame or PFrame until finished
        while sender.is_finished().await.is_none() {
            let buf = match stream.next().await {
                Some(Ok(StampedData { data, .. })) => data,
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
        let _ = sender.join().await;
    } else {
        // Simply use the snap command
        debug!("Using the snap command");
        let file_path = opt.file_path.with_extension("jpeg");
        let mut buffer = File::create(file_path).await?;
        let jpeg_data = camera
            .run_task(|camera| Box::pin(async move { Ok(camera.get_snapshot().await?) }))
            .await?;
        buffer.write_all(jpeg_data.as_slice()).await?;
    }

    Ok(())
}

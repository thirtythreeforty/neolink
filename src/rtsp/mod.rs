///
/// # Neolink RTSP
///
/// This module serves the rtsp streams for the
/// `neolink rtsp` subcommand
///
/// All camera specified in the config.toml will be served
/// over rtsp. By default it bind to all local ip addresses
/// on the port 8554.
///
/// You can view the streams with any rtsp compliement program
/// such as ffmpeg, vlc, blue-iris, home-assistant, zone-minder etc.
///
/// Each camera has it own endpoint based on its name. For example
/// a camera named `"Garage"` in the config can be found at.
///
/// `rtsp://my.ip.address:8554/Garage`
///
/// With the lower resolution stream at
///
/// `rtsp://my.ip.address:8554/Garage/subStream`
///
/// # Usage
///
/// To start the subcommand use the following in a shell.
///
/// ```bash
/// neolink rtsp --config=config.toml
/// ```
///
/// # Example Config
///
/// ```toml
// [[cameras]]
// name = "Cammy"
// username = "****"
// password = "****"
// address = "****:9000"
//   [cameras.pause]
//   on_motion = false
//   on_client = false
//   mode = "none"
//   timeout = 1.0
// ```
//
// - When `on_motion` is true the camera will pause streaming when motion is stopped and resume it when motion is started
// - When `on_client` is true the camera will pause while there is no client connected.
// - `timeout` handels how long to wait after motion stops before pausing the stream
// - `mode` has the following values:
//   - `"black"`: Switches to a black screen. Requires more cpu as the stream is fully reencoded
//   - `"still"`: Switches to a still image. Requires more cpu as the stream is fully reencoded
//   - `"test"`: Switches to the gstreamer test image. Requires more cpu as the stream is fully reencoded
//   - `"none"`: Resends the last iframe the camera. This does not reencode at all.  **Most use cases should use this one as it has the least effort on the cpu and gives what you would expect**
//
use anyhow::{anyhow, Context, Result};
use log::*;
use std::sync::Arc;
use std::time::Duration;

mod cmdline;
mod gst;
mod states;

use super::config::Config;
pub(crate) use cmdline::Opt;
use gst::NeoRtspServer;
use states::*;

/// Entry point for the rtsp subcommand
///
/// Opt is the command line options
pub(crate) async fn main(_opt: Opt, mut config: Config) -> Result<()> {
    let rtsp = Arc::new(NeoRtspServer::new()?);

    rtsp.set_up_tls(&config);

    rtsp.set_up_users(&config.users);

    if config.certificate.is_none() && !config.users.is_empty() {
        warn!(
            "Without a server certificate, usernames and passwords will be exchanged in plaintext!"
        )
    }
    let mut cameras = vec![];
    for camera_config in config.cameras.drain(..) {
        cameras
            .push(Camera::<Disconnected>::new(camera_config, &config.users, rtsp.clone()).await?);
    }

    let mut set = tokio::task::JoinSet::new();
    for camera in cameras.drain(..) {
        // Spawn each camera controller in it's own thread
        set.spawn(async move {
            let name = camera.get_name();
            loop {
                let failure = camera_main(camera).await;
                match failure {
                    Err(CameraFailureKind::Fatal(e)) => {
                        error!("{}: Fatal error: {:?}", name, e);
                        return Err(e);
                    }
                    Err(CameraFailureKind::Retry(e)) => {
                        warn!("{}: Retryable error: {:X?}", name, e);
                        todo!(); // Do backoff
                    }
                    Ok(()) => {
                        info!("{}: Shutting down", name);
                        break;
                    }
                }
            }
            Ok(())
        });
    }
    info!(
        "Starting RTSP Server at {}:{}",
        &config.bind_addr, config.bind_port,
    );

    let bind_addr = config.bind_addr.clone();
    let bind_port = config.bind_port;
    set.spawn(async move { rtsp.run(&bind_addr, bind_port).await });

    if let Some(joined) = set.join_next().await {
        joined??
    }

    Ok(())
}

enum CameraFailureKind {
    Fatal(anyhow::Error),
    Retry(anyhow::Error),
}

async fn camera_main(camera: Camera<Disconnected>) -> Result<(), CameraFailureKind> {
    // Connect
    let name = camera.get_name();
    let connected = camera
        .connect()
        .await
        .with_context(|| format!("{}: Could not connect to camera", name))
        .map_err(CameraFailureKind::Retry)?;

    let loggedin = connected
        .login()
        .await
        .with_context(|| format!("{}: Could not login to camera", name))
        .map_err(CameraFailureKind::Fatal)?;

    let _ = loggedin.manage().await;

    let mut streaming = loggedin
        .stream()
        .await
        .with_context(|| format!("{}: Could not start stream", name))
        .map_err(CameraFailureKind::Retry)?;

    tokio::time::sleep(Duration::from_secs(2)).await; // Wait for a few seconds of video before we allow pausing

    loop {
        // Wait for error or reason to pause
        tokio::select! {
            v = async {
                // Wait for error
                streaming.join().await
            } => v,
            v = async {
                // Wait for motion stop
                let mut motion = streaming.get_camera().listen_on_motion().await?;
                motion.await_stop(Duration::from_secs_f64(streaming.get_config().pause.motion_timeout)).await
            }, if streaming.get_config().pause.on_motion => {
                v.map_err(|e| anyhow!("Error while processing motion messages: {:?}", e))
            },
            v = async {
                // Wait for client to disconnect
                todo!()
            }, if streaming.get_config().pause.on_disconnect => v,
        }.with_context(|| format!("{}: Error while streaming", name))
        .map_err(CameraFailureKind::Retry)?;

        let paused = streaming
            .stop()
            .await
            .with_context(|| format!("{}: Could not stop stream", name))
            .map_err(CameraFailureKind::Retry)?;
        // Wait for reason to restart
        tokio::select! {
            v = async {
                // Wait for motion start
                let mut motion = paused.get_camera().listen_on_motion().await?;
                motion.await_start(Duration::ZERO).await
            }, if paused.get_config().pause.on_motion => {
                v.map_err(|e| anyhow!("Error while processing motion messages: {:?}", e))
            },
            v = async {
                // Wait for client to connect
                todo!()
            }, if paused.get_config().pause.on_disconnect => v,
            else => {
                // No pause. This means that the stream stopped for some reason
                // but not because of an error
                Ok(())
            }
        }
        .with_context(|| format!("{}: Error while paused", name))
        .map_err(CameraFailureKind::Retry)?;

        streaming = paused
            .stream()
            .await
            .with_context(|| format!("{}: Could not start stream", name))
            .map_err(CameraFailureKind::Retry)?;
    }
}

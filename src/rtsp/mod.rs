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
use futures::stream::FuturesUnordered;
use log::*;
use neolink_core::bc_protocol::{BcCamera, StreamKind};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::StreamExt;

mod cmdline;
mod gst;
mod spring;
mod states;

use super::config::Config;
pub(crate) use cmdline::Opt;
use gst::NeoRtspServer;
pub(crate) use spring::*;
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
    for mut camera in cameras.drain(..) {
        // Spawn each camera controller in it's own thread
        set.spawn(async move {
            let shared = camera.shared.clone();
            let name = camera.get_name();
            let mut backoff = Duration::from_micros(125);
            loop {
                tokio::task::yield_now().await;
                let failure = camera_main(camera).await;
                match failure {
                    Err(CameraFailureKind::Fatal(e)) => {
                        error!("{}: Fatal error: {:?}", name, e);
                        return Err(e);
                    }
                    Err(CameraFailureKind::Retry(e)) => {
                        warn!("{}: Retryable error: {:X?}", name, e);
                        tokio::time::sleep(backoff).await;
                        if backoff < Duration::from_secs(5) {
                            backoff *= 2;
                        }
                        camera = Camera {
                            shared: shared.clone(),
                            state: Disconnected {},
                        };
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
    rtsp.run(&bind_addr, bind_port).await?;
    let thread_rtsp = rtsp.clone();
    set.spawn(async move { thread_rtsp.join().await });

    while let Some(joined) = set.join_next().await {
        match &joined {
            Err(_) | Ok(Err(_)) => {
                // Panicked or error in task
                rtsp.quit().await?;
            }
            Ok(Ok(_)) => {
                // All good
            }
        }
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
        .map_err(|e| {
            let e_inner = e.downcast_ref::<neolink_core::Error>().unwrap();
            match e_inner {
                neolink_core::Error::CameraLoginFail => CameraFailureKind::Fatal(e),
                _ => CameraFailureKind::Retry(e),
            }
        })?;

    let _ = loggedin.manage().await;

    let tags = loggedin.shared.get_tags();
    let rtsp_thread = loggedin.get_rtsp();

    // Clear all buffers present
    // Uncomment to clear buffers. This is now handlled in the buffer itself,
    // instead of clearing it restamps it whenever there is a jump in the
    // timestamps of >1s
    //
    // tags.iter()
    //     .map(|tag| rtsp_thread.clear_buffer(tag))
    //     .collect::<FuturesUnordered<_>>()
    //     .collect::<Vec<_>>()
    //     .await;

    // Start pulling data from the camera
    let mut streaming = loggedin
        .stream()
        .await
        .with_context(|| format!("{}: Could not start stream", name))
        .map_err(CameraFailureKind::Retry)?;

    // Wait for buffers to be prepared
    tokio::select! {
        v = async {
            let mut waiter = tokio::time::interval(Duration::from_micros(500));
            loop {
                waiter.tick().await;
                if tags
                    .iter()
                    .map(|tag| rtsp_thread.buffer_ready(tag))
                    .collect::<FuturesUnordered<_>>()
                    .all(|f| f.unwrap_or(false))
                    .await
                {
                    break;
                }
            }
            Ok(())
        } => v,
        // Or for stream to error
        v = streaming.join() => {v},
    }
    .with_context(|| format!("{}: Error while waiting for buffers", name))
    .map_err(CameraFailureKind::Retry)?;

    tags.iter()
        .map(|tag| rtsp_thread.jump_to_live(tag))
        .collect::<FuturesUnordered<_>>()
        .collect::<Vec<_>>()
        .await;

    // Clear "stream not ready" media to try and force a reconnect
    //   This shoud stop them from watching the "Stream Not Ready" thing
    debug!("Clearing not ready clients");
    tags.iter()
        .map(|tag| rtsp_thread.clear_session_notready(tag))
        .collect::<FuturesUnordered<_>>()
        .collect::<Vec<_>>()
        .await;
    log::info!("{}: Buffers prepared", name);

    let mut active_tags = streaming.shared.get_streams().clone();
    let mut motion_pause = false;
    loop {
        // Wait for error or reason to pause
        let change = tokio::select! {
            v = async {
            // Wait for error
            streaming.join().await
            }, if ! active_tags.is_empty() => {
                info!("{}: Join Pause", name);
                Ok(StreamChange::StreamError(v))
            },
            v = await_change(
                streaming.get_camera(),
                &streaming.shared,
                &rtsp_thread,
                &active_tags,
                motion_pause,
                &name,
            ), if streaming.shared.get_config().pause.on_motion || streaming.shared.get_config().pause.on_disconnect => {
                v.with_context(|| format!("{}: Error updating pause state", name))
                .map_err(CameraFailureKind::Retry)
            }
        }?;

        match change {
            StreamChange::StreamError(res) => {
                res.map_err(CameraFailureKind::Retry)?;
            }
            StreamChange::MotionStart => {
                motion_pause = false;
                let inactive_streams = streaming
                    .shared
                    .get_streams()
                    .iter()
                    .filter(|i| !active_tags.contains(i))
                    .copied()
                    .collect::<Vec<_>>();

                if streaming.shared.get_config().pause.on_disconnect {
                    // Pause on client is also on
                    //
                    // Only resume clients with active connections
                    for stream in inactive_streams.iter() {
                        if rtsp_thread
                            .get_number_of_clients(streaming.shared.get_tag_for_stream(stream))
                            .await
                            .map(|n| n > 0)
                            .unwrap_or(false)
                        {
                            streaming
                                .start_stream(*stream)
                                .await
                                .map_err(CameraFailureKind::Retry)?;
                            active_tags.insert(*stream);
                            rtsp_thread
                                .resume(streaming.shared.get_tag_for_stream(stream))
                                .await
                                .map_err(CameraFailureKind::Retry)?;
                        }
                    }
                } else {
                    // Pause on client is not on
                    //
                    // Resume all
                    for stream in inactive_streams.iter() {
                        streaming
                            .start_stream(*stream)
                            .await
                            .map_err(CameraFailureKind::Retry)?;
                        active_tags.insert(*stream);
                        rtsp_thread
                            .resume(streaming.shared.get_tag_for_stream(stream))
                            .await
                            .map_err(CameraFailureKind::Retry)?;
                    }
                }
            }
            StreamChange::MotionStop => {
                motion_pause = true;
                // Clear all streams
                for stream in active_tags.drain() {
                    rtsp_thread
                        .pause(streaming.shared.get_tag_for_stream(&stream))
                        .await
                        .map_err(CameraFailureKind::Retry)?;
                    streaming
                        .stop_stream(stream)
                        .await
                        .map_err(CameraFailureKind::Retry)?;
                }
            }
            StreamChange::ClientStart(stream) => {
                if !streaming.shared.get_config().pause.on_motion || !motion_pause {
                    streaming
                        .start_stream(stream)
                        .await
                        .map_err(CameraFailureKind::Retry)?;
                    active_tags.insert(stream);
                    rtsp_thread
                        .resume(streaming.shared.get_tag_for_stream(&stream))
                        .await
                        .map_err(CameraFailureKind::Retry)?;
                }
            }
            StreamChange::ClientStop(stream) => {
                if !streaming.shared.get_config().pause.on_motion || !motion_pause {
                    rtsp_thread
                        .pause(streaming.shared.get_tag_for_stream(&stream))
                        .await
                        .map_err(CameraFailureKind::Retry)?;
                    streaming
                        .stop_stream(stream)
                        .await
                        .map_err(CameraFailureKind::Retry)?;
                    active_tags.remove(&stream);
                }
            }
        }
    }
    // Ok(())
}

enum StreamChange {
    StreamError(Result<()>),
    MotionStart,
    MotionStop,
    ClientStart(StreamKind),
    ClientStop(StreamKind),
}
async fn await_change(
    camera: &BcCamera,
    shared: &Shared,
    rtsp_thread: &NeoRtspServer,
    active_tags: &HashSet<StreamKind>,
    motion_pause: bool,
    name: &str,
) -> Result<StreamChange> {
    tokio::select! {
            v = async {
                // Wait for motion stop
                let mut motion = camera.listen_on_motion().await?;
                motion.await_stop(Duration::from_secs_f64(shared.get_config().pause.motion_timeout)).await
            }, if motion_pause && shared.get_config().pause.on_motion => {
                info!("{}: Motion Pause", name);
                v.map_err(|e| anyhow!("Error while processing motion messages: {:?}", e))?;
                Ok(StreamChange::MotionStop)
            },
            v = async {
                // Wait for client to disconnect
                let mut inter = tokio::time::interval(tokio::time::Duration::from_secs_f32(0.01));

                loop {
                    inter.tick().await;
                    for tag in active_tags.iter() {
                        if rtsp_thread.get_number_of_clients(shared.get_tag_for_stream(tag)).await.map(|n| n == 0).unwrap_or(true) {
                            return Result::<_,anyhow::Error>::Ok(StreamChange::ClientStop(*tag))
                        }
                    }
                }
            }, if shared.get_config().pause.on_disconnect => {
                if let Ok(StreamChange::ClientStop(tag)) = v.as_ref() {
                    info!("{}: Client Pause:: {:?}", name, tag);
                }
                v
            },
            v = async {
                // Wait for motion start
                let mut motion = camera.listen_on_motion().await?;
                motion.await_start(Duration::ZERO).await
            }, if ! motion_pause && shared.get_config().pause.on_motion => {
                info!("{}: Motion Resume", name);
                v.with_context(|| "Error while processing motion messages")?;
                Ok(StreamChange::MotionStart)
            },
            v = async {
                // Wait for client to connect
                let mut inter = tokio::time::interval(tokio::time::Duration::from_secs_f32(0.01));
                let inactive_tags = shared.get_streams().iter().filter(|i| !active_tags.contains(i)).collect::<Vec<_>>();
                trace!("inactive_tags: {:?}", inactive_tags);
                loop {
                    inter.tick().await;
                    for tag in inactive_tags.iter() {
                        if rtsp_thread.get_number_of_clients(shared.get_tag_for_stream(tag)).await.map(|n| n > 0).unwrap_or(false) {
                            return Result::<_,anyhow::Error>::Ok(StreamChange::ClientStart(**tag))
                        }
                    }
                }
            }, if shared.get_config().pause.on_disconnect => {
                if let Ok(StreamChange::ClientStart(tag)) = v.as_ref() {
                    info!("{}: Client Resume:: {:?}", name, tag);
                }
                v
            },
        }.with_context(|| format!("{}: Error while streaming", name))
}

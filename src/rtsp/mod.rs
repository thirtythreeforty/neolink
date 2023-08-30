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
use gstreamer_rtsp_server::prelude::*;
use log::*;
use neolink_core::bc_protocol::{BcCamera, StreamKind};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use tokio_util::sync::CancellationToken;

mod cmdline;
mod gst;
mod spring;

use crate::{
    common::{NeoInstance, NeoReactor, VidFormat},
    rtsp::gst::NeoMediaFactory,
};

use super::config::Config;
pub(crate) use cmdline::Opt;
use gst::NeoRtspServer;
pub(crate) use spring::*;

type AnyResult<T> = anyhow::Result<T, anyhow::Error>;

/// Entry point for the rtsp subcommand
///
/// Opt is the command line options
pub(crate) async fn main(_opt: Opt, mut config: Config, reactor: NeoReactor) -> Result<()> {
    let rtsp = Arc::new(NeoRtspServer::new()?);

    rtsp.set_up_tls(&config);

    rtsp.set_up_users(&config.users);

    if config.certificate.is_none() && !config.users.is_empty() {
        warn!(
            "Without a server certificate, usernames and passwords will be exchanged in plaintext!"
        )
    }
    let mut cameras = vec![];
    for camera_config in config.cameras.drain(..).filter(|c| c.enabled) {
        cameras.push(reactor.get_or_insert(camera_config).await?);
    }

    let global_cancel = CancellationToken::new();

    let mut set = tokio::task::JoinSet::new();
    for mut camera in cameras.drain(..) {
        let stream_info = camera
            .run_task(|cam| Box::pin(async move { Ok(cam.get_stream_info().await?) }))
            .await?;

        let supported_streams = {
            stream_info
                .stream_infos
                .iter()
                .flat_map(|stream_info| stream_info.encode_tables.clone())
                .flat_map(|encode| match encode.name.as_str() {
                    "mainStream" => Some(StreamKind::Main),
                    "subStream" => Some(StreamKind::Sub),
                    "externStream" => Some(StreamKind::Extern),
                    new_stream_name => {
                        log::debug!("New stream name {}", new_stream_name);
                        None
                    }
                })
                .collect::<HashSet<_>>()
        };

        let thread_global_cancel = global_cancel.clone();
        let thread_rtsp = rtsp.clone();
        set.spawn(async move {
            tokio::select!(
                _ = thread_global_cancel.cancelled() => {
                    AnyResult::Ok(())
                },
                v = async {
                    let mut camera_config = camera.config().await?.clone();
                    loop {
                        let prev_stream_config = camera_config.borrow_and_update().stream.clone();
                        let active_streams = prev_stream_config.to_stream_kinds().drain(..).collect::<HashSet<_>>();

                        // This select is for changes to camera_config.stream
                        break tokio::select!{
                            v = camera_config.wait_for(|config| config.stream != prev_stream_config) => {
                                if let Err(e) = v {
                                    AnyResult::Err(e.into())
                                } else {
                                    // config.stream changed restart
                                    continue;
                                }
                            },
                            v = async {
                                // This select handles enabling the right stream
                                tokio::select! {
                                    v = async {
                                        let mut stream_instance = camera.stream(StreamKind::Main).await?;
                                        loop {
                                            // Wait for a valid stream format to be detected
                                            stream_instance.config.wait_for(|config| !matches!(config.vid_format, VidFormat::None) && config.resolution[0] > 0 && config.resolution[1] > 0).await?;
                                            let stream = stream_instance.stream.resubscribe();
                                            let stream_name = match stream_instance.name {
                                                StreamKind::Main => "main",
                                                StreamKind::Sub => "sub",
                                                StreamKind::Extern => "extern",
                                            }
                                            .to_string();
                                            // This select ensures is for the streams config
                                            break tokio::select!{
                                                _ = stream_instance.config.changed() => {
                                                    // If stream config changes we reload the stream
                                                    continue;
                                                },
                                                v = async {
                                                    // Finally ready to create the factory and connect the stream
                                                    let mounts = thread_rtsp
                                                        .mount_points()
                                                        .ok_or(anyhow!("RTSP server lacks mount point"))?;
                                                    let name = camera.config().await?.borrow().name.clone();
                                                    let path = format!("/{}/{}", name, stream_name);
                                                    log::info!("Path: {}", path);
                                                    let factory = NeoMediaFactory::new_with_callback(move |element| {
                                                        let stream_data = &stream;
                                                        Ok(Some(element))
                                                    }).await?;
                                                    factory.add_permitted_roles(&["anonymous"].into());
                                                    mounts.add_factory(&path, factory);
                                                    AnyResult::Ok(())
                                                } => v,
                                            };
                                        }
                                     }, if active_streams.contains(&StreamKind::Main) && supported_streams.contains(&StreamKind::Main) => v,
                                     else => {
                                         futures::pending!();
                                         AnyResult::Ok(())
                                     }
                                }
                            } => v,
                        };
                    }
                } => v,
            )
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

async fn camera_main(camera: NeoInstance, rtsp: NeoRtspServer) -> Result<(), CameraFailureKind> {
    // Connect
    let config = camera
        .config()
        .await
        .map_err(CameraFailureKind::Fatal)?
        .borrow_and_update()
        .clone();
    let name = config.name;

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
    // let mut streaming = loggedin
    //     .stream()
    //     .await
    //     .with_context(|| format!("{}: Could not start stream", name))
    //     .map_err(CameraFailureKind::Retry)?;

    // // Wait for buffers to be prepared
    // tokio::select! {
    //     v = async {
    //         let mut waiter = tokio::time::interval(Duration::from_micros(500));
    //         loop {
    //             waiter.tick().await;
    //             if tags
    //                 .iter()
    //                 .map(|tag| rtsp_thread.buffer_ready(tag))
    //                 .collect::<FuturesUnordered<_>>()
    //                 .all(|f| f.unwrap_or(false))
    //                 .await
    //             {
    //                 break;
    //             }
    //         }
    //         Ok(())
    //     } => v,
    //     // Or for stream to error
    //     v = streaming.join() => {v},
    // }
    // .with_context(|| format!("{}: Error while waiting for buffers", name))
    // .map_err(CameraFailureKind::Retry)?;

    // tags.iter()
    //     .map(|tag| rtsp_thread.jump_to_live(tag))
    //     .collect::<FuturesUnordered<_>>()
    //     .collect::<Vec<_>>()
    //     .await;

    // // Clear "stream not ready" media to try and force a reconnect
    // //   This shoud stop them from watching the "Stream Not Ready" thing
    // debug!("Clearing not ready clients");
    // tags.iter()
    //     .map(|tag| rtsp_thread.clear_session_notready(tag))
    //     .collect::<FuturesUnordered<_>>()
    //     .collect::<Vec<_>>()
    //     .await;
    // log::info!("{}: Buffers prepared", name);

    // let mut active_tags = streaming.shared.get_streams().clone();
    // let mut motion_pause = false;
    // loop {
    //     // Wait for error or reason to pause
    //     let change = tokio::select! {
    //         v = async {
    //         // Wait for error
    //         streaming.join().await
    //         }, if ! active_tags.is_empty() => {
    //             info!("{}: Join Pause", name);
    //             Ok(StreamChange::StreamError(v))
    //         },
    //         // Send pings
    //         v = async {
    //             let camera = streaming.get_camera();
    //             let mut interval = IntervalStream::new(interval(Duration::from_secs(5)));
    //             while let Some(_update) = interval.next().await {
    //                 if camera.ping().await.is_err() {
    //                     break;
    //                 }
    //             }
    //             futures::pending!(); // Never actually finish, has to be aborted
    //             Ok(())
    //         } => Ok(StreamChange::StreamError(v)),
    //         v = await_change(
    //             streaming.get_camera(),
    //             &streaming.shared,
    //             &rtsp_thread,
    //             &active_tags,
    //             motion_pause,
    //             &name,
    //         ), if streaming.shared.get_config().pause.on_motion || streaming.shared.get_config().pause.on_disconnect => {
    //             v.with_context(|| format!("{}: Error updating pause state", name))
    //             .map_err(CameraFailureKind::Retry)
    //         }
    //     }?;

    //     match change {
    //         StreamChange::StreamError(res) => {
    //             res.map_err(CameraFailureKind::Retry)?;
    //         }
    //         StreamChange::MotionStart => {
    //             motion_pause = false;
    //             let inactive_streams = streaming
    //                 .shared
    //                 .get_streams()
    //                 .iter()
    //                 .filter(|i| !active_tags.contains(i))
    //                 .copied()
    //                 .collect::<Vec<_>>();

    //             if streaming.shared.get_config().pause.on_disconnect {
    //                 // Pause on client is also on
    //                 //
    //                 // Only resume clients with active connections
    //                 for stream in inactive_streams.iter() {
    //                     if rtsp_thread
    //                         .get_number_of_clients(streaming.shared.get_tag_for_stream(stream))
    //                         .await
    //                         .map(|n| n > 0)
    //                         .unwrap_or(false)
    //                     {
    //                         streaming
    //                             .start_stream(*stream)
    //                             .await
    //                             .map_err(CameraFailureKind::Retry)?;
    //                         active_tags.insert(*stream);
    //                         rtsp_thread
    //                             .resume(streaming.shared.get_tag_for_stream(stream))
    //                             .await
    //                             .map_err(CameraFailureKind::Retry)?;
    //                     }
    //                 }
    //             } else {
    //                 // Pause on client is not on
    //                 //
    //                 // Resume all
    //                 for stream in inactive_streams.iter() {
    //                     streaming
    //                         .start_stream(*stream)
    //                         .await
    //                         .map_err(CameraFailureKind::Retry)?;
    //                     active_tags.insert(*stream);
    //                     rtsp_thread
    //                         .resume(streaming.shared.get_tag_for_stream(stream))
    //                         .await
    //                         .map_err(CameraFailureKind::Retry)?;
    //                 }
    //             }
    //         }
    //         StreamChange::MotionStop => {
    //             motion_pause = true;
    //             // Clear all streams
    //             for stream in active_tags.drain() {
    //                 rtsp_thread
    //                     .pause(streaming.shared.get_tag_for_stream(&stream))
    //                     .await
    //                     .map_err(CameraFailureKind::Retry)?;
    //                 streaming
    //                     .stop_stream(stream)
    //                     .await
    //                     .map_err(CameraFailureKind::Retry)?;
    //             }
    //         }
    //         StreamChange::ClientStart(stream) => {
    //             if !streaming.shared.get_config().pause.on_motion || !motion_pause {
    //                 streaming
    //                     .start_stream(stream)
    //                     .await
    //                     .map_err(CameraFailureKind::Retry)?;
    //                 active_tags.insert(stream);
    //                 rtsp_thread
    //                     .resume(streaming.shared.get_tag_for_stream(&stream))
    //                     .await
    //                     .map_err(CameraFailureKind::Retry)?;
    //             }
    //         }
    //         StreamChange::ClientStop(stream) => {
    //             if !streaming.shared.get_config().pause.on_motion || !motion_pause {
    //                 rtsp_thread
    //                     .pause(streaming.shared.get_tag_for_stream(&stream))
    //                     .await
    //                     .map_err(CameraFailureKind::Retry)?;
    //                 streaming
    //                     .stop_stream(stream)
    //                     .await
    //                     .map_err(CameraFailureKind::Retry)?;
    //                 active_tags.remove(&stream);
    //             }
    //         }
    //     }
    // }
    Ok(())
}

enum StreamChange {
    StreamError(Result<()>),
    MotionStart,
    MotionStop,
    ClientStart(StreamKind),
    ClientStop(StreamKind),
}
// async fn await_change(
//     camera: &BcCamera,
//     shared: &Shared,
//     rtsp_thread: &NeoRtspServer,
//     active_tags: &HashSet<StreamKind>,
//     motion_pause: bool,
//     name: &str,
// ) -> Result<StreamChange> {
//     tokio::select! {
//             v = async {
//                 // Wait for motion stop
//                 let mut motion = camera.listen_on_motion().await?;
//                 motion.await_stop(Duration::from_secs_f64(shared.get_config().pause.motion_timeout)).await
//             }, if !motion_pause && shared.get_config().pause.on_motion => {
//                 info!("{}: Motion Pause", name);
//                 v.map_err(|e| anyhow!("Error while processing motion messages: {:?}", e))?;
//                 Ok(StreamChange::MotionStop)
//             },
//             v = async {
//                 // Wait for client to disconnect
//                 let mut inter = tokio::time::interval(tokio::time::Duration::from_secs_f32(0.01));

//                 loop {
//                     inter.tick().await;
//                     for tag in active_tags.iter() {
//                         if rtsp_thread.get_number_of_clients(shared.get_tag_for_stream(tag)).await.map(|n| n == 0).unwrap_or(true) {
//                             return Result::<_,anyhow::Error>::Ok(StreamChange::ClientStop(*tag))
//                         }
//                     }
//                 }
//             }, if shared.get_config().pause.on_disconnect => {
//                 if let Ok(StreamChange::ClientStop(tag)) = v.as_ref() {
//                     info!("{}: Client Pause:: {:?}", name, tag);
//                 }
//                 v
//             },
//             v = async {
//                 // Wait for motion start
//                 let mut motion = camera.listen_on_motion().await?;
//                 motion.await_start(Duration::ZERO).await
//             }, if motion_pause && shared.get_config().pause.on_motion => {
//                 info!("{}: Motion Resume", name);
//                 v.with_context(|| "Error while processing motion messages")?;
//                 Ok(StreamChange::MotionStart)
//             },
//             v = async {
//                 // Wait for client to connect
//                 let mut inter = tokio::time::interval(tokio::time::Duration::from_secs_f32(0.01));
//                 let inactive_tags = shared.get_streams().iter().filter(|i| !active_tags.contains(i)).collect::<Vec<_>>();
//                 trace!("inactive_tags: {:?}", inactive_tags);
//                 loop {
//                     inter.tick().await;
//                     for tag in inactive_tags.iter() {
//                         if rtsp_thread.get_number_of_clients(shared.get_tag_for_stream(tag)).await.map(|n| n > 0).unwrap_or(false) {
//                             return Result::<_,anyhow::Error>::Ok(StreamChange::ClientStart(**tag))
//                         }
//                     }
//                 }
//             }, if shared.get_config().pause.on_disconnect => {
//                 if let Ok(StreamChange::ClientStart(tag)) = v.as_ref() {
//                     info!("{}: Client Resume:: {:?}", name, tag);
//                 }
//                 v
//             },
//         }.with_context(|| format!("{}: Error while streaming", name))
// }

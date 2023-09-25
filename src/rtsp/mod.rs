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
use gstreamer::{Bin, Caps, ClockTime, Element, ElementFactory};
use gstreamer_app::{AppSrc, AppSrcCallbacks, AppStreamType};
use gstreamer_rtsp_server::prelude::*;
use log::*;
use neolink_core::bc_protocol::StreamKind;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::{
    sync::{
        broadcast::channel as broadcast,
        mpsc::{channel as mpsc, Receiver as MpscReceiver, Sender as MpscSender},
        watch::channel as watch,
    },
    task::JoinSet,
    time::{interval, sleep, Duration, Instant},
};
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use tokio_util::sync::CancellationToken;

mod cmdline;
mod gst;

use crate::common::{Permit, UseCounter};
use crate::{
    common::{AudFormat, NeoInstance, NeoReactor, StreamConfig, StreamInstance, VidFormat},
    rtsp::gst::NeoMediaFactory,
};

use super::config::UserConfig;
pub(crate) use cmdline::Opt;
use gst::NeoRtspServer;

type AnyResult<T> = anyhow::Result<T, anyhow::Error>;

/// Entry point for the rtsp subcommand
///
/// Opt is the command line options
pub(crate) async fn main(_opt: Opt, reactor: NeoReactor) -> Result<()> {
    let rtsp = Arc::new(NeoRtspServer::new()?);

    let global_cancel = CancellationToken::new();

    let mut set = JoinSet::new();

    // Thread for the TLS from the config
    let mut thread_config = reactor.config().await?;
    let thread_cancel = global_cancel.clone();
    let thread_rtsp = rtsp.clone();
    thread_rtsp.set_up_tls(&thread_config.borrow_and_update().clone())?;
    set.spawn(async move {
        tokio::select! {
            _ = thread_cancel.cancelled() => AnyResult::Ok(()),
            v = async {
                loop {
                    thread_config.changed().await?;
                    if let Err(e) = thread_rtsp.set_up_tls(&thread_config.borrow().clone()) {
                        log::error!("Could not seup TLS: {e}");
                    }
                }
            } => v
        }
    });

    // Thread for the Users from the config
    let mut thread_config = reactor.config().await?;
    let thread_cancel = global_cancel.clone();
    let thread_rtsp = rtsp.clone();
    set.spawn(async move {
        tokio::select! {
            _ = thread_cancel.cancelled() => AnyResult::Ok(()),
            v = async {
                let mut curr_users = HashSet::new();
                loop {

                    curr_users = thread_config.wait_for(|new_config|
                        new_config.users.iter().cloned().collect::<HashSet<_>>() != curr_users
                    ).await?.users.iter().cloned().collect::<HashSet<_>>();

                    let config = thread_config.borrow().clone();
                    if let Err(e) = apply_users(&thread_rtsp, &curr_users).await {
                        log::error!("Could not seup TLS: {e}");
                    }

                    if config.certificate.is_none() && !curr_users.is_empty() {
                        warn!(
                            "Without a server certificate, usernames and passwords will be exchanged in plaintext!"
                        )
                    }
                }
            } => v
        }
    });

    // Startup and stop cameras as they are added/removed to the config
    let mut thread_config = reactor.config().await?;
    let thread_cancel = global_cancel.clone();
    let thread_rtsp = rtsp.clone();
    let thread_reactor = reactor.clone();
    set.spawn(async move {
        let mut set = JoinSet::<AnyResult<()>>::new();
        let thread_cancel2 = thread_cancel.clone();
        tokio::select!{
            _ = thread_cancel.cancelled() => AnyResult::Ok(()),
            v = async {
                let mut cameras: HashMap<String, CancellationToken> = Default::default();
                let mut config_names = HashSet::new();
                loop {
                    config_names = thread_config.wait_for(|config| {
                        let current_names = config.cameras.iter().filter(|a| a.enabled).map(|cam_config| cam_config.name.clone()).collect::<HashSet<_>>();
                        current_names != config_names
                    }).await.with_context(|| "Camera Config Watcher")?.clone().cameras.iter().filter(|a| a.enabled).map(|cam_config| cam_config.name.clone()).collect::<HashSet<_>>();

                    for name in config_names.iter() {
                        if ! cameras.contains_key(name) {
                            log::info!("{name}: Rtsp Staring");
                            let local_cancel = CancellationToken::new();
                            cameras.insert(name.clone(),local_cancel.clone() );
                            let thread_global_cancel = thread_cancel2.clone();
                            let thread_rtsp2 = thread_rtsp.clone();
                            let thread_reactor2 = thread_reactor.clone();
                            let name = name.clone();
                            set.spawn(async move {
                                let camera = thread_reactor2.get(&name).await?;
                                tokio::select!(
                                    _ = thread_global_cancel.cancelled() => {
                                        AnyResult::Ok(())
                                    },
                                    _ = local_cancel.cancelled() => {
                                        AnyResult::Ok(())
                                    },
                                    v = camera_main(camera, &thread_rtsp2) => v,
                                )
                            }) ;
                        }
                    }

                    for (running_name, token) in cameras.iter() {
                        if ! config_names.contains(running_name) {
                            log::debug!("Rtsp::main Cancel1");
                            token.cancel();
                        }
                    }
                }
            } => v,
        }
    });

    let rtsp_config = reactor.config().await?.borrow().clone();
    info!(
        "Starting RTSP Server at {}:{}",
        &rtsp_config.bind_addr, rtsp_config.bind_port,
    );

    let bind_addr = rtsp_config.bind_addr.clone();
    let bind_port = rtsp_config.bind_port;
    rtsp.run(&bind_addr, bind_port).await?;
    let thread_rtsp = rtsp.clone();
    set.spawn(async move { thread_rtsp.join().await });

    while let Some(joined) = set
        .join_next()
        .await
        .map(|s| s.map_err(anyhow::Error::from))
    {
        match &joined {
            Err(e) | Ok(Err(e)) => {
                // Panicked or error in task
                // Cancel all and await terminate
                log::error!("Error: {e}");
                log::debug!("Rtsp::main Cancel2");
                global_cancel.cancel();
                rtsp.quit().await?;
            }
            Ok(Ok(_)) => {
                // All good
            }
        }
    }

    Ok(())
}

/// This keeps the users in rtsp and the config in sync
async fn apply_users(rtsp: &NeoRtspServer, curr_users: &HashSet<UserConfig>) -> AnyResult<()> {
    // Add those missing
    for user in curr_users.iter() {
        log::debug!("Adding user {} to rtsp server", user.name);
        rtsp.add_user(&user.name, &user.pass).await?;
    }
    // Remove unused
    let rtsp_users = rtsp.get_users().await?;
    for user in rtsp_users {
        if !curr_users.iter().any(|a| a.name == user) {
            log::debug!("Removing user {} from rtsp server", user);
            rtsp.remove_user(&user).await?;
        }
    }
    Ok(())
}

/// Top level camera entry point
///
/// It checks which streams are supported and then starts them
async fn camera_main(camera: NeoInstance, rtsp: &NeoRtspServer) -> Result<()> {
    let name = camera.config().await?.borrow().name.clone();
    log::debug!("{name}: Camera Main");
    let later_camera = camera.clone();
    let (supported_streams_tx, supported_streams) = watch(HashSet::<StreamKind>::new());

    let mut set = JoinSet::new();
    set.spawn(async move {
        let mut i = IntervalStream::new(interval(Duration::from_secs(15)));
        while i.next().await.is_some() {
            let stream_info = later_camera
                .run_task(|cam| Box::pin(async move { Ok(cam.get_stream_info().await?) }))
                .await?;

            let new_supported_streams = stream_info
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
                .collect::<HashSet<_>>();
            supported_streams_tx.send_if_modified(|old| {
                if *old != new_supported_streams {
                    *old = new_supported_streams;
                    true
                } else {
                    false
                }
            });
        }
        AnyResult::Ok(())
    });

    log::debug!("{name}: Camera Main::Loop");

    let mut camera_config = camera.config().await?.clone();
    loop {
        let prev_stream_config = camera_config.borrow_and_update().stream;
        let prev_stream_users = camera_config.borrow().permitted_users.clone();
        let active_streams = prev_stream_config
            .as_stream_kinds()
            .drain(..)
            .collect::<HashSet<_>>();
        let use_splash = camera_config.borrow().use_splash;

        // This select is for changes to camera_config.stream
        break tokio::select! {
            v = camera_config.wait_for(|config| config.stream != prev_stream_config || config.permitted_users != prev_stream_users || config.use_splash != use_splash) => {
                if let Err(e) = v {
                    AnyResult::Err(e.into())
                } else {
                    // config.stream changed restart
                    continue;
                }
            },
            v = async {
                // This select handles enabling the right stream
                log::debug!("{name}: Camera Main::Select Stream");
                // and setting up the users
                let all_users = rtsp.get_users().await?.iter().filter(|a| *a != "anyone" && *a != "anonymous").cloned().collect::<HashSet<_>>();
                let permitted_users: HashSet<String> = match &prev_stream_users {
                    // If in the camera config there is the user "anyone", or if none is specified but users
                    // are defined at all, then we add all users to the camera's allowed list.
                    Some(p) if p.iter().any(|u| u == "anyone") => all_users,
                    None if !all_users.is_empty() => all_users,

                    // The user specified permitted_users
                    Some(p) => p.iter().cloned().collect(),

                    // The user didn't specify permitted_users, and there are none defined anyway
                    None => ["anonymous".to_string()].iter().cloned().collect(),
                };

                // Create the dummy factory
                let dummy_factory = NeoMediaFactory::new_with_callback(move |element| {
                    clear_bin(&element)?;
                    if !use_splash {
                        Ok(None)
                    } else {
                        build_unknown(&element)?;
                        Ok(Some(element))
                    }
                })
                .await?;
                dummy_factory.add_permitted_roles(&permitted_users);
                let mut supported_streams_1 = supported_streams.clone();
                let mut supported_streams_2 = supported_streams.clone();
                let mut supported_streams_3 = supported_streams.clone();
                tokio::select! {
                    v = async {
                        log::debug!("{name}: Camera Main::Select Main");
                        let name = camera.config().await?.borrow().name.clone();
                        let mut paths = vec![
                            format!("/{name}/main"),
                            format!("/{name}/Main"),
                            format!("/{name}/mainStream"),
                            format!("/{name}/MainStream"),
                            format!("/{name}/Mainstream"),
                            format!("/{name}/mainstream"),
                        ];
                        paths.push(
                            format!("/{name}")
                        );
                        // Create a dummy factory so that the URL will not return 404 while waiting
                        // for configuration to compete
                        //
                        // This is for BI since it will give up forever on a 404 rather then retry
                        //
                        let mounts = rtsp
                            .mount_points()
                            .ok_or(anyhow!("RTSP server lacks mount point"))?;
                        for path in paths.iter() {
                            log::debug!("Path: {}", path);
                            mounts.add_factory(path, dummy_factory.clone());
                        }
                        log::debug!("{}: Preparing at {}", name, paths.join(", "));

                        supported_streams_1.wait_for(|ss| ss.contains(&StreamKind::Main)).await?;
                        stream_main(camera.stream(StreamKind::Main).await?, camera.clone(), rtsp, &permitted_users, &paths).await
                    }, if active_streams.contains(&StreamKind::Main) => v,
                    v = async {
                        log::debug!("{name}: Camera Main::Select Sub");
                        let name = camera.config().await?.borrow().name.clone();
                        let mut paths = vec![
                            format!("/{name}/sub"),
                            format!("/{name}/Sub"),
                            format!("/{name}/subStream"),
                            format!("/{name}/SubStream"),
                            format!("/{name}/Substream"),
                            format!("/{name}/substream"),
                        ];
                        if ! active_streams.contains(&StreamKind::Main) {
                            paths.push(
                                format!("/{name}")
                            );
                        }

                        // Create a dummy factory so that the URL will not return 404 while waiting
                        // for configuration to compete
                        //
                        // This is for BI since it will give up forever on a 404 rather then retry
                        //
                        let mounts = rtsp
                            .mount_points()
                            .ok_or(anyhow!("RTSP server lacks mount point"))?;
                        // Create the dummy factory
                        for path in paths.iter() {
                            log::debug!("Path: {}", path);
                            mounts.add_factory(path, dummy_factory.clone());
                        }
                        log::debug!("{}: Preparing at {}", name, paths.join(", "));

                        supported_streams_2.wait_for(|ss| ss.contains(&StreamKind::Sub)).await?;
                        stream_main(camera.stream(StreamKind::Sub).await?,camera.clone(), rtsp, &permitted_users, &paths).await
                    }, if active_streams.contains(&StreamKind::Sub) => v,
                    v = async {
                        log::debug!("{name}: Camera Main::Select Extern");
                        let name = camera.config().await?.borrow().name.clone();
                        let mut paths = vec![
                            format!("/{name}/extern"),
                            format!("/{name}/Extern"),
                            format!("/{name}/externStream"),
                            format!("/{name}/ExternStream"),
                            format!("/{name}/Externstream"),
                            format!("/{name}/externstream"),
                        ];
                        if ! active_streams.contains(&StreamKind::Main) && ! active_streams.contains(&StreamKind::Sub) {
                            paths.push(
                                format!("/{name}")
                            );
                        }

                        // Create a dummy factory so that the URL will not return 404 while waiting
                        // for configuration to compete
                        //
                        // This is for BI since it will give up forever on a 404 rather then retry
                        //
                        let mounts = rtsp
                            .mount_points()
                            .ok_or(anyhow!("RTSP server lacks mount point"))?;
                        for path in paths.iter() {
                            log::debug!("Path: {}", path);
                            mounts.add_factory(path, dummy_factory.clone());
                        }
                        log::debug!("{}: Preparing at {}", name, paths.join(", "));

                        supported_streams_3.wait_for(|ss| ss.contains(&StreamKind::Extern)).await?;
                        stream_main(camera.stream(StreamKind::Extern).await?,camera.clone(), rtsp, &permitted_users, &paths).await
                    }, if active_streams.contains(&StreamKind::Extern) => v,
                    else => {
                        // all disabled just wait here until config is changed
                        futures::future::pending().await
                    }
                }
            } => v,
        };
    }?;

    Ok(())
}

#[derive(Clone, Debug)]
enum StreamData {
    Media { data: Arc<Vec<u8>>, ts: Duration },
    Seek { ts: Duration, reply: MpscSender<()> },
}

struct ClientSourceData {
    app: AppSrc,
    msg: MpscReceiver<StreamData>,
}

struct ClientData {
    vid: Option<ClientSourceData>,
    aud: Option<ClientSourceData>,
}

/// This handles the stream itself by creating the factory and pushing messages into it
async fn stream_main(
    mut stream_instance: StreamInstance,
    camera: NeoInstance,
    rtsp: &NeoRtspServer,
    users: &HashSet<String>,
    paths: &[String],
) -> Result<()> {
    let mut camera_config = camera.config().await?.clone();
    let name = camera_config.borrow().name.clone();

    let mut curr_pause;
    loop {
        log::debug!("{}: Activating Stream", &name);
        stream_instance.activate().await?;

        // Wait for a valid stream format to be detected
        log::debug!("{}: Waiting for Valid Stream", &name);
        stream_instance
            .config
            .wait_for(|config| {
                log::debug!("{:?}", config);
                !matches!(config.vid_format, VidFormat::None)
                    && config.resolution[0] > 0
                    && config.resolution[1] > 0
            })
            .await?;
        log::debug!("{}: Waiting for Valid Audio", &name);
        // After vid give it 1s to look for audio
        // Ignore timeout but check err
        if let Ok(v) = tokio::time::timeout(
            Duration::from_secs(1),
            stream_instance.config.wait_for(|config| {
                log::debug!("{:?}", config);
                !matches!(config.vid_format, VidFormat::None)
                    && !matches!(config.aud_format, AudFormat::None)
                    && config.resolution[0] > 0
                    && config.resolution[1] > 0
            }),
        )
        .await
        {
            v?;
        }

        curr_pause = camera_config.borrow().pause.clone();

        let last_stream_config = stream_instance.config.borrow().clone();
        let mut thread_stream_config = stream_instance.config.clone();

        let mut set = JoinSet::<AnyResult<()>>::new();
        log::debug!("{}: Creating Client Counters", &name);
        // Handles the on off of the stream with the client pause
        let client_counter = UseCounter::new().await;
        let client_count = client_counter.create_deactivated().await?;
        if curr_pause.on_disconnect {
            log::debug!("{}: Enabling Client Pause", &name);
            // Take over activation
            let mut client_activator = stream_instance.activator_handle().await;
            client_activator.activate().await?;
            stream_instance.deactivate().await?;
            let client_count = client_counter.create_deactivated().await?;
            let thread_name = name.clone();
            set.spawn(async move {
                loop {
                    log::info!("{}: Activating Client", thread_name);
                    client_activator.activate().await?;
                    client_count.dropped_users().await?;
                    log::info!("{}: Pausing Client", thread_name);
                    client_activator.deactivate().await?;
                    client_count.aquired_users().await?;
                }
            });
        }

        // Handles on motion pausing
        if curr_pause.on_motion {
            log::debug!("{}: Activating Motion Pause", &name);
            // Take over activation
            let mut client_activator = stream_instance.activator_handle().await;
            stream_instance.deactivate().await?;
            let mut motion = camera.motion().await?;
            let delta = Duration::from_secs_f64(curr_pause.motion_timeout);
            let thread_name = name.clone();
            set.spawn(async move {
                loop {
                    log::info!("{}: Enabling Motion", thread_name);
                    client_activator.activate().await?;
                    motion.wait_for(|md| matches!(md, crate::common::MdState::Stop(_))).await?;
                    log::info!("{}: Pausing Motion", thread_name);
                    client_activator.deactivate().await?;
                    motion.wait_for(|md| matches!(md, crate::common::MdState::Start(n) if (*n - Instant::now())>delta)).await?;
                }
            });
        }

        // This thread jsut keeps it active for 2s after an initial start to build the buffer
        let mut init_activator = stream_instance.activator_handle().await;
        set.spawn(async move {
            init_activator.activate().await?;
            sleep(Duration::from_secs(2)).await;
            init_activator.deactivate().await?;
            AnyResult::Ok(())
        });

        // This runs the actual stream.
        // The select will restart if the stream's config updates
        log::debug!("{}: Stream Activated", &name);
        break tokio::select! {
            v = thread_stream_config.wait_for(|new_conf| new_conf != &last_stream_config) => {
                let v = v?;
                // If stream config changes we reload the stream
                log::info!("{}: Stream Configuration Changed. Reloading Streams", &name);
                log::trace!("    From {:?} to {:?}", last_stream_config, v.clone());
                continue;
            },
            v = camera_config.wait_for(|new_conf| new_conf.pause != curr_pause ) => {
                v?;
                // If pause config changes restart
                log::info!("{}: Pause Configuration Changed. Reloading Streams", &name);
                continue;
            },
            v = stream_run(&name, &stream_instance, rtsp, &last_stream_config, users, paths, client_count) => v,
        };
    }
}

async fn stream_run(
    name: &str,
    stream_instance: &StreamInstance,
    rtsp: &NeoRtspServer,
    stream_config: &StreamConfig,
    users: &HashSet<String>,
    paths: &[String],
    client_count: Permit,
) -> AnyResult<()> {
    let vidstream = stream_instance.vid.resubscribe();
    let audstream = stream_instance.aud.resubscribe();
    let vid_history = stream_instance.vid_history.clone();
    let aud_history = stream_instance.aud_history.clone();

    // Finally ready to create the factory and connect the stream
    let mounts = rtsp
        .mount_points()
        .ok_or(anyhow!("RTSP server lacks mount point"))?;
    // Create the factory
    let (factory, mut client_rx) = make_factory(stream_config).await?;

    factory.add_permitted_roles(users);

    for path in paths.iter() {
        log::debug!("Path: {}", path);
        mounts.add_factory(path, factory.clone());
    }
    log::info!("{}: Avaliable at {}", name, paths.join(", "));

    let stream_cancel = CancellationToken::new();
    let drop_guard = stream_cancel.clone().drop_guard();
    let mut set = JoinSet::new();
    // Wait for new media client data to come in from the factory
    while let Some(mut client_data) = client_rx.recv().await {
        log::trace!("New media");
        // New media created
        let (vid, mut vid_seek) = client_data
            .vid
            .take()
            .map_or((None, None), |data| (Some(data.app), Some(data.msg)));
        let (aud, mut aud_seek) = client_data
            .aud
            .take()
            .map_or((None, None), |data| (Some(data.app), Some(data.msg)));

        // This is the data that gets sent to gstreamer thread
        // It represents the combination of the camera stream and the appsrc seek messages
        let (aud_data_tx, aud_data_rx) = broadcast(100);
        let (vid_data_tx, vid_data_rx) = broadcast(100);

        // This thread takes the video data from the cam and passed it into the stream
        let mut vidstream = BroadcastStream::new(vidstream.resubscribe());
        let thread_vid_data_tx = vid_data_tx.clone();
        let thread_stream_cancel = stream_cancel.clone();
        let thread_vid_history = vid_history.clone();
        set.spawn(async move {
            let r = tokio::select! {
                _ = thread_stream_cancel.cancelled() => AnyResult::Ok(()),
                v = async {
                    // Send Initial
                    let mut found_key_frame = false;
                    for data in thread_vid_history.borrow().iter() {
                        if data.keyframe || found_key_frame {
                            found_key_frame = true;
                            thread_vid_data_tx.send(
                                StreamData::Media {
                                    data: data.data.clone(),
                                    ts: Duration::ZERO,
                                }
                            )?;
                        }
                    }

                    // Send new
                    while let Some(data) = vidstream.next().await {
                        if let Ok(data) = data {
                            thread_vid_data_tx.send(
                                StreamData::Media {
                                    data: data.data,
                                    ts: data.ts
                                }
                            )?;
                        }
                        // Ignore broadcast lag errors
                    }
                    AnyResult::Ok(())
                } => v
            };
            log::trace!("Stream Vid Media End {r:?}");
            AnyResult::Ok(())
        });

        // This thread takes the audio data from the cam and passed it into the stream
        let mut audstream = BroadcastStream::new(audstream.resubscribe());
        let thread_stream_cancel = stream_cancel.clone();
        let thread_aud_data_tx = aud_data_tx.clone();
        let thread_aud_history = aud_history.clone();
        set.spawn(async move {
            let r = tokio::select! {
                _ = thread_stream_cancel.cancelled() => AnyResult::Ok(()),
                v = async {
                    // Send Initial
                    let mut found_key_frame = false;
                    for data in thread_aud_history.borrow().iter() {
                        if data.keyframe || found_key_frame {
                            thread_aud_data_tx.send(
                                StreamData::Media {
                                    data: data.data.clone(),
                                    ts: Duration::ZERO,
                                }
                            )?;
                            found_key_frame = true;
                        }
                    }
                    // Send new
                    while let Some(data) = audstream.next().await {
                        if let Ok(data) = data {
                            thread_aud_data_tx.send(
                                StreamData::Media {
                                    data: data.data,
                                    ts: data.ts
                                }
                            )?;
                        }
                        // Ignore broadcast lag errors
                    }
                    AnyResult::Ok(())
                } => v,
            };
            log::trace!("Stream Aud Media End: {r:?}");
            AnyResult::Ok(())
        });

        // This thread takes the seek data for the vid and passed it into the stream
        let thread_stream_cancel = stream_cancel.clone();
        let thread_vid_history = vid_history.clone();
        set.spawn(async move {
            let r = tokio::select! {
                _ = thread_stream_cancel.cancelled() => AnyResult::Ok(()),
                v = async {
                    if let Some(vid_seek) = vid_seek.as_mut() {
                        while let Some(data) = vid_seek.recv().await {
                            let seek_ts = if let StreamData::Seek{ts, ..} = &data {
                                Some(*ts)
                            } else {
                                None
                            };
                            // Send seek
                            vid_data_tx.send(data)?;
                            // Send initial buffer
                            if let Some(seek_ts) = seek_ts {
                                // Send Initial
                                for data in thread_vid_history.borrow().iter() {
                                    vid_data_tx.send(
                                        StreamData::Media {
                                            data: data.data.clone(),
                                            ts: seek_ts,
                                        }
                                    )?;
                                }
                            }
                        }
                    }
                    AnyResult::Ok(())
                } => v,
            };
            log::trace!("Stream Vid Seek End: {r:?}");
            r
        });

        // This thread takes the seek data for the aud and passed it into the stream
        let thread_stream_cancel = stream_cancel.clone();
        let thread_aud_history = aud_history.clone();
        set.spawn(async move {
            let r = tokio::select! {
                _ = thread_stream_cancel.cancelled() => AnyResult::Ok(()),
                v = async {
                    if let Some(aud_seek) = aud_seek.as_mut() {
                        while let Some(data) = aud_seek.recv().await {
                            let seek_ts = if let StreamData::Seek{ts, ..} = &data {
                                Some(*ts)
                            } else {
                                None
                            };
                            aud_data_tx.send(data)?;
                            // Send initial buffer
                            if let Some(seek_ts) = seek_ts {
                                // Send Initial
                                for data in thread_aud_history.borrow().iter() {
                                    aud_data_tx.send(
                                        StreamData::Media {
                                            data: data.data.clone(),
                                            ts: seek_ts,
                                        }
                                    )?;
                                }
                            }
                        }
                    }
                    AnyResult::Ok(())
                } => v
            };
            log::trace!("Stream Aud Seek End: {r:?}");
            r
        });

        // Handles sending the video data into gstreamer
        let thread_stream_cancel = stream_cancel.clone();
        let vid_data_rx = BroadcastStream::new(vid_data_rx).filter(|f| f.is_ok()); // Filter to ignore lagged
        let thread_vid = vid.clone();
        let mut thread_client_count = client_count.subscribe();
        set.spawn(async move {
            thread_client_count.activate().await?;
            let r = tokio::select! {
                _ = thread_stream_cancel.cancelled() => {
                    AnyResult::Ok(())
                },
                v = handle_data(thread_vid.as_ref(), vid_data_rx) => v,
            };
            drop(thread_client_count);
            log::trace!("Vid Thread End: {:?}", r);
            r
        });

        // Handles the audio data into gstreamer
        let thread_stream_cancel = stream_cancel.clone();
        let aud_data_rx = BroadcastStream::new(aud_data_rx).filter(|f| f.is_ok()); // Filter to ignore lagged
        let thread_aud = aud.clone();
        set.spawn(async move {
            let r = tokio::select! {
                _ = thread_stream_cancel.cancelled() => {
                    AnyResult::Ok(())
                },
                v = handle_data(thread_aud.as_ref(), aud_data_rx) => v,
            };
            log::trace!("Aud Thread End: {:?}", r);
            r
        });
    }
    // At this point the factory has been destroyed
    // Cancel any remaining threads that are trying to send data
    // Although it should be finished already when the appsrcs are dropped
    stream_cancel.cancel();
    drop(drop_guard);
    while set.join_next().await.is_some() {}
    log::trace!("Stream done");
    AnyResult::Ok(())
}

async fn make_factory(
    stream_config: &StreamConfig,
) -> AnyResult<(NeoMediaFactory, MpscReceiver<ClientData>)> {
    let (client_tx, client_rx) = mpsc(100);
    let factory = {
        let stream_config = stream_config.clone();
        NeoMediaFactory::new_with_callback(move |element| {
            clear_bin(&element)?;
            let (vid_seek_tx, vid_seek_rx) = mpsc(10);
            let (aud_seek_tx, aud_seek_rx) = mpsc(10);
            let vid = match stream_config.vid_format {
                VidFormat::None => {
                    build_unknown(&element)?;
                    AnyResult::Ok(None)
                }
                VidFormat::H264 => {
                    let app = build_h264(&element)?;
                    app.set_callbacks(
                        AppSrcCallbacks::builder()
                            .seek_data(move |_, seek_pos| {
                                log::trace!("seek_pos: {seek_pos:?}");
                                let (reply_tx, mut reply_rx) = mpsc(1);
                                vid_seek_tx
                                    .blocking_send(StreamData::Seek {
                                        ts: Duration::from_micros(seek_pos),
                                        reply: reply_tx,
                                    })
                                    .is_ok()
                                    && reply_rx.blocking_recv().is_some()
                            })
                            .build(),
                    );
                    AnyResult::Ok(Some(app))
                }
                VidFormat::H265 => {
                    let app = build_h265(&element)?;
                    app.set_callbacks(
                        AppSrcCallbacks::builder()
                            .seek_data(move |_, seek_pos| {
                                log::trace!("seek_pos: {seek_pos:?}");
                                let (reply_tx, mut reply_rx) = mpsc(1);
                                vid_seek_tx
                                    .blocking_send(StreamData::Seek {
                                        ts: Duration::from_micros(seek_pos),
                                        reply: reply_tx,
                                    })
                                    .is_ok()
                                    && reply_rx.blocking_recv().is_some()
                            })
                            .build(),
                    );
                    AnyResult::Ok(Some(app))
                }
            }?;
            let aud = if matches!(stream_config.vid_format, VidFormat::None) {
                None
            } else {
                match stream_config.aud_format {
                    AudFormat::None => AnyResult::Ok(None),
                    AudFormat::Aac => {
                        let app = build_aac(&element)?;
                        app.set_callbacks(
                            AppSrcCallbacks::builder()
                                .seek_data(move |_, seek_pos| {
                                    log::trace!("seek_pos: {seek_pos:?}");
                                    let (reply_tx, mut reply_rx) = mpsc(1);
                                    aud_seek_tx
                                        .blocking_send(StreamData::Seek {
                                            ts: Duration::from_micros(seek_pos),
                                            reply: reply_tx,
                                        })
                                        .is_ok()
                                        && reply_rx.blocking_recv().is_some()
                                })
                                .build(),
                        );
                        AnyResult::Ok(Some(app))
                    }
                    AudFormat::Adpcm(block_size) => {
                        let app = build_adpcm(&element, block_size)?;
                        app.set_callbacks(
                            AppSrcCallbacks::builder()
                                .seek_data(move |_, seek_pos| {
                                    log::trace!("seek_pos: {seek_pos:?}");
                                    let (reply_tx, mut reply_rx) = mpsc(1);
                                    aud_seek_tx
                                        .blocking_send(StreamData::Seek {
                                            ts: Duration::from_micros(seek_pos),
                                            reply: reply_tx,
                                        })
                                        .is_ok()
                                        && reply_rx.blocking_recv().is_some()
                                })
                                .build(),
                        );
                        AnyResult::Ok(Some(app))
                    }
                }?
            };

            client_tx.blocking_send(ClientData {
                vid: match (vid, vid_seek_rx) {
                    (Some(app), msg) => Some(ClientSourceData { app, msg }),
                    _ => None,
                },
                aud: match (aud, aud_seek_rx) {
                    (Some(app), msg) => Some(ClientSourceData { app, msg }),
                    _ => None,
                },
            })?;
            Ok(Some(element))
        })
        .await
    }?;

    Ok((factory, client_rx))
}

async fn handle_data<T: Stream<Item = Result<StreamData, E>> + Unpin, E>(
    app: Option<&AppSrc>,
    mut data_rx: T,
) -> Result<()> {
    if let Some(app) = app {
        let mut last_ft: Option<Duration> = None;
        let mut ft = Duration::ZERO;
        let mut last_rt: Option<Duration> = None;
        let mut rt = Duration::ZERO;
        let (gst_data_tx, mut gst_data_rx) = mpsc(100);
        let appsrc = app.clone();
        // push_buffer can block so we need to do this on a dedicated thread
        // however it is expensive to call spawn_blocking too often
        // so we call it once and push via a channel
        tokio::task::spawn_blocking(move || {
            while let Some(buf) = gst_data_rx.blocking_recv() {
                appsrc
                    .push_buffer(buf)
                    .map(|_| ())
                    .map_err(|_| anyhow!("Could not push buffer to appsrc"))?;
            }
            AnyResult::Ok(())
        });
        while let Some(Ok(data)) = data_rx.next().await {
            check_live(app)?; // Stop if appsrc is dropped
                              // Cache the last runtime
            match data {
                StreamData::Seek { ts, reply } => {
                    rt = ts;
                    last_rt = None;
                    let _ = reply.send(()).await;
                }
                StreamData::Media { data, ts: ft_i } => {
                    log::trace!("Frame recieved with ts: {ft_i:?}");
                    // Update rt
                    if let Some(rt_i) = get_runtime(app) {
                        if let Some(last_rt) = last_rt {
                            let delta_rt = rt_i.saturating_sub(last_rt);
                            rt += delta_rt;
                        }
                        last_rt = Some(rt_i);
                    }
                    // Update ft
                    if let Some(last_ft) = last_ft {
                        let delta_ft = ft_i.saturating_sub(last_ft);
                        ft += delta_ft;
                    }
                    last_ft = Some(ft_i);

                    // Sync ft to rt if > 1500ms difference
                    const MAX_DELTA_T: Duration = Duration::from_millis(1500);
                    let delta_t = if rt > ft { rt - ft } else { ft - rt };
                    if delta_t > MAX_DELTA_T {
                        ft = rt;
                    }

                    let buf = {
                        let mut gst_buf = gstreamer::Buffer::with_size(data.len()).unwrap();
                        {
                            let gst_buf_mut = gst_buf.get_mut().unwrap();
                            log::trace!("Setting PTS: {ft:?}, Runtime: {rt:?}");
                            let time = ClockTime::from_useconds(ft.as_micros() as u64);
                            gst_buf_mut.set_dts(time);
                            gst_buf_mut.set_pts(time);
                            let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
                            gst_buf_data.copy_from_slice(data.as_slice());
                        }
                        gst_buf
                    };

                    gst_data_tx.send(buf).await?;
                }
            }
        }
    }
    Ok(())
}

fn check_live(app: &AppSrc) -> Result<()> {
    app.pads()
        .iter()
        .all(|pad| pad.is_linked())
        .then_some(())
        .ok_or(anyhow!("App source is closed"))
}

fn get_runtime(app: &AppSrc) -> Option<Duration> {
    if let Some(clock) = app.clock() {
        if let Some(time) = clock.time() {
            if let Some(base_time) = app.base_time() {
                let runtime = time.saturating_sub(base_time);
                return Some(Duration::from_micros(runtime.useconds()));
            }
        }
    }
    None
}

fn clear_bin(bin: &Element) -> Result<()> {
    let bin = bin
        .clone()
        .dynamic_cast::<Bin>()
        .map_err(|_| anyhow!("Media source's element should be a bin"))?;
    // Clear the autogenerated ones
    log::debug!("Clearing old elements");
    for element in bin.iterate_elements().into_iter().flatten() {
        bin.remove(&element)?;
    }

    Ok(())
}

fn build_unknown(bin: &Element) -> Result<()> {
    let bin = bin
        .clone()
        .dynamic_cast::<Bin>()
        .map_err(|_| anyhow!("Media source's element should be a bin"))?;
    log::debug!("Building Unknown Pipeline");
    let source = make_element("videotestsrc", "testvidsrc")?;
    source.set_property_from_str("pattern", "snow");
    source.set_property("num-buffers", 500i32); // Send buffers then EOS
    let queue = make_queue("queue0")?;

    let overlay = make_element("textoverlay", "overlay")?;
    overlay.set_property("text", "Stream not Ready");
    overlay.set_property_from_str("valignment", "top");
    overlay.set_property_from_str("halignment", "left");
    overlay.set_property("font-desc", "Sans, 16");
    let encoder = make_element("jpegenc", "encoder")?;
    let payload = make_element("rtpjpegpay", "pay0")?;

    bin.add_many(&[&source, &queue, &overlay, &encoder, &payload])?;
    source.link_filtered(
        &queue,
        &Caps::builder("video/x-raw")
            .field("format", "YUY2")
            .field("width", 896i32)
            .field("height", 512i32)
            .field("framerate", gstreamer::Fraction::new(25, 1))
            .build(),
    )?;
    Element::link_many(&[&queue, &overlay, &encoder, &payload])?;

    Ok(())
}

fn build_h264(bin: &Element) -> Result<AppSrc> {
    let bin = bin
        .clone()
        .dynamic_cast::<Bin>()
        .map_err(|_| anyhow!("Media source's element should be a bin"))?;
    log::debug!("Building H264 Pipeline");
    let source = make_element("appsrc", "vidsrc")?
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow!("Cannot cast to appsrc."))?;

    source.set_is_live(false);
    source.set_block(false);
    source.set_property("emit-signals", false);
    source.set_max_bytes(50000000u64); // 50MB
    source.set_do_timestamp(false);
    source.set_stream_type(AppStreamType::Seekable);

    let source = source
        .dynamic_cast::<Element>()
        .map_err(|_| anyhow!("Cannot cast back"))?;
    let queue = make_queue("source_queue")?;
    let parser = make_element("h264parse", "parser")?;
    let payload = make_element("rtph264pay", "pay0")?;
    bin.add_many(&[&source, &queue, &parser, &payload])?;
    Element::link_many(&[&source, &queue, &parser, &payload])?;

    let source = source
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow!("Cannot convert appsrc"))?;
    Ok(source)
}

fn build_h265(bin: &Element) -> Result<AppSrc> {
    let bin = bin
        .clone()
        .dynamic_cast::<Bin>()
        .map_err(|_| anyhow!("Media source's element should be a bin"))?;
    log::debug!("Building H265 Pipeline");
    let source = make_element("appsrc", "vidsrc")?
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow!("Cannot cast to appsrc."))?;
    source.set_is_live(false);
    source.set_block(false);
    source.set_property("emit-signals", false);
    source.set_max_bytes(52428800);
    source.set_do_timestamp(false);
    source.set_stream_type(AppStreamType::Seekable);

    let source = source
        .dynamic_cast::<Element>()
        .map_err(|_| anyhow!("Cannot cast back"))?;
    let queue = make_queue("source_queue")?;
    let parser = make_element("h265parse", "parser")?;
    let payload = make_element("rtph265pay", "pay0")?;
    bin.add_many(&[&source, &queue, &parser, &payload])?;
    Element::link_many(&[&source, &queue, &parser, &payload])?;

    let source = source
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow!("Cannot convert appsrc"))?;
    Ok(source)
}

fn build_aac(bin: &Element) -> Result<AppSrc> {
    let bin = bin
        .clone()
        .dynamic_cast::<Bin>()
        .map_err(|_| anyhow!("Media source's element should be a bin"))?;
    log::debug!("Building Aac pipeline");
    let source = make_element("appsrc", "audsrc")?
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow!("Cannot cast to appsrc."))?;

    source.set_is_live(false);
    source.set_block(false);
    source.set_property("emit-signals", false);
    source.set_max_bytes(52428800);
    source.set_do_timestamp(false);
    source.set_stream_type(AppStreamType::Seekable);

    let source = source
        .dynamic_cast::<Element>()
        .map_err(|_| anyhow!("Cannot cast back"))?;

    let queue = make_queue("audqueue")?;
    let parser = make_element("aacparse", "audparser")?;
    let decoder = match make_element("faad", "auddecoder_faad") {
        Ok(ele) => Ok(ele),
        Err(_) => make_element("avdec_aac", "auddecoder_avdec_aac"),
    }?;
    let encoder = make_element("audioconvert", "audencoder")?;
    let payload = make_element("rtpL16pay", "pay1")?;

    bin.add_many(&[&source, &queue, &parser, &decoder, &encoder, &payload])?;
    Element::link_many(&[&source, &queue, &parser, &decoder, &encoder, &payload])?;

    let source = source
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow!("Cannot convert appsrc"))?;
    Ok(source)
}

fn build_adpcm(bin: &Element, block_size: u32) -> Result<AppSrc> {
    let bin = bin
        .clone()
        .dynamic_cast::<Bin>()
        .map_err(|_| anyhow!("Media source's element should be a bin"))?;
    log::debug!("Building Adpcm pipeline");
    // Original command line
    // caps=audio/x-adpcm,layout=dvi,block_align={},channels=1,rate=8000
    // ! queue silent=true max-size-bytes=10485760 min-threshold-bytes=1024
    // ! adpcmdec
    // ! audioconvert
    // ! rtpL16pay name=pay1

    let source = make_element("appsrc", "audsrc")?
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow!("Cannot cast to appsrc."))?;
    source.set_is_live(false);
    source.set_block(false);
    source.set_property("emit-signals", false);
    source.set_max_bytes(52428800);
    source.set_do_timestamp(false);
    source.set_stream_type(AppStreamType::Seekable);

    source.set_caps(Some(
        &Caps::builder("audio/x-adpcm")
            .field("layout", "div")
            .field("block_align", block_size as i32)
            .field("channels", 1i32)
            .field("rate", 8000i32)
            .build(),
    ));

    let source = source
        .dynamic_cast::<Element>()
        .map_err(|_| anyhow!("Cannot cast back"))?;

    let queue = make_queue("audqueue")?;
    let decoder = make_element("decodebin", "auddecoder")?;
    let encoder = make_element("audioconvert", "audencoder")?;
    let payload = make_element("rtpL16pay", "pay1")?;

    bin.add_many(&[&source, &queue, &decoder, &encoder, &payload])?;
    Element::link_many(&[&source, &queue, &decoder])?;
    Element::link_many(&[&encoder, &payload])?;
    decoder.connect_pad_added(move |_element, pad| {
        debug!("Linking encoder to decoder: {:?}", pad.caps());
        let sink_pad = encoder
            .static_pad("sink")
            .expect("Encoder is missing its pad");
        pad.link(&sink_pad)
            .expect("Failed to link ADPCM decoder to encoder");
    });

    let source = source
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow!("Cannot convert appsrc"))?;
    Ok(source)
}

// Convenice funcion to make an element or provide a message
// about what plugin is missing
fn make_element(kind: &str, name: &str) -> AnyResult<Element> {
    ElementFactory::make_with_name(kind, Some(name)).with_context(|| {
        let plugin = match kind {
            "appsrc" => "app (gst-plugins-base)",
            "audioconvert" => "audioconvert (gst-plugins-base)",
            "adpcmdec" => "Required for audio",
            "h264parse" => "videoparsersbad (gst-plugins-bad)",
            "h265parse" => "videoparsersbad (gst-plugins-bad)",
            "rtph264pay" => "rtp (gst-plugins-good)",
            "rtph265pay" => "rtp (gst-plugins-good)",
            "rtpjitterbuffer" => "rtp (gst-plugins-good)",
            "aacparse" => "audioparsers (gst-plugins-good)",
            "rtpL16pay" => "rtp (gst-plugins-good)",
            "x264enc" => "x264 (gst-plugins-ugly)",
            "x265enc" => "x265 (gst-plugins-bad)",
            "avdec_h264" => "libav (gst-libav)",
            "avdec_h265" => "libav (gst-libav)",
            "videotestsrc" => "videotestsrc (gst-plugins-base)",
            "imagefreeze" => "imagefreeze (gst-plugins-good)",
            "audiotestsrc" => "audiotestsrc (gst-plugins-base)",
            "decodebin" => "playback (gst-plugins-good)",
            _ => "Unknown",
        };
        format!(
            "Missing required gstreamer plugin `{}` for `{}` element",
            plugin, kind
        )
    })
}
fn make_queue(name: &str) -> AnyResult<Element> {
    // let queue = make_element("queue", name)?;
    // queue.set_property_from_str("leaky", "downstream");
    // queue.set_property("max-size-bytes", 0u32);
    // queue.set_property("max-size-buffers", 0u32);
    // queue.set_property(
    //     "max-size-time",
    //     std::convert::TryInto::<u64>::try_into(tokio::time::Duration::from_secs(5).as_nanos())
    //         .unwrap_or(0),
    // );
    // Ok(queue)

    let queue = make_element("queue2", name)?;
    // queue.set_property("max-size-bytes", 0u32);
    // queue.set_property("max-size-buffers", 0u32);
    queue.set_property(
        "max-size-time",
        std::convert::TryInto::<u64>::try_into(tokio::time::Duration::from_secs(5).as_nanos())
            .unwrap_or(0),
    );
    queue.set_property("use-buffering", true);
    Ok(queue)
}

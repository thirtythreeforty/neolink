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
use gstreamer_rtsp_server::prelude::*;
use log::*;
use neolink_core::bc_protocol::StreamKind;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::{
    sync::watch::channel as watch,
    task::JoinSet,
    time::{interval, Duration},
};
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

mod cmdline;
mod factory;
mod gst;
mod stream;

use crate::common::{NeoInstance, NeoReactor};
use factory::*;
use stream::*;

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
                .run_passive_task(|cam| Box::pin(async move { Ok(cam.get_stream_info().await?) }))
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
                let dummy_factory = make_dummy_factory(use_splash).await?;
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

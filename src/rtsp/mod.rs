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
use crossbeam::utils::Backoff;
use log::*;
use std::sync::Arc;
use std::time::Duration;

mod abort;
mod cmdline;
mod gst;
mod states;

use abort::AbortHandle;
use states::{RtspCamera, StateInfo};

use super::config::{CameraConfig, Config, UserConfig};
pub(crate) use cmdline::Opt;
use gst::{RtspServer, TlsAuthenticationMode};

/// Entry point for the rtsp subcommand
///
/// Opt is the command line options
pub(crate) async fn main(_opt: Opt, mut config: Config) -> Result<()> {
    let rtsp = Arc::new(RtspServer::new()?);

    set_up_tls(&config, &rtsp);

    set_up_users(&config.users, &rtsp);

    if config.certificate.is_none() && !config.users.is_empty() {
        warn!(
            "Without a server certificate, usernames and passwords will be exchanged in plaintext!"
        )
    }
    let mut set = tokio::task::JoinSet::new();
    let abort = AbortHandle::new();

    let arc_user_config = Arc::new(config.users);
    for camera_config in config.cameras.drain(..) {
        // Spawn each camera controller in it's own thread
        let user_config = arc_user_config.clone();
        let arc_rtsp = rtsp.clone();
        let abort_handle = abort.clone();
        set.spawn(async move {
            let backoff = Backoff::new();

            while abort_handle.is_live() {
                let failure =
                    camera_main(&camera_config, user_config.as_slice(), arc_rtsp.clone()).await;
                match failure {
                    Err(CameraFailureKind::Fatal(e)) => {
                        error!("{}: Fatal error: {:?}", camera_config.name, e);
                        break;
                    }
                    Err(CameraFailureKind::Retry(e)) => {
                        warn!("{}: Retryable error: {:X?}", camera_config.name, e);
                        backoff.spin();
                    }
                    Ok(()) => {
                        info!("{}: Shutting down", camera_config.name);
                        break;
                    }
                }
            }
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
        joined?
    }

    Ok(())
}

enum CameraFailureKind {
    Fatal(anyhow::Error),
    Retry(anyhow::Error),
}

async fn camera_main(
    config: &CameraConfig,
    user_config: &[UserConfig],
    rtsp: Arc<RtspServer>,
) -> Result<(), CameraFailureKind> {
    // Connect
    let mut camera = RtspCamera::new(config, user_config, rtsp)
        .await
        .with_context(|| format!("{}: Could not connect camera", config.name))
        .map_err(CameraFailureKind::Retry)?;
    camera
        .login()
        .await
        .with_context(|| format!("{}: Could not login to camera", config.name))
        .map_err(CameraFailureKind::Fatal)?;

    let _ = camera.manage().await;

    info!("Init Stream");
    camera
        .stream()
        .await
        .with_context(|| format!("{}: Could not start stream", config.name))
        .map_err(CameraFailureKind::Retry)?;
    info!("Inited Stream");

    let backoff = Backoff::new();
    let mut motion = if config.pause.on_motion {
        Some(
            camera
                .motion_data()
                .await
                .with_context(|| {
                    "Could not initialise motion detection. Is it supported with this camera?"
                })
                .map_err(CameraFailureKind::Retry)?,
        )
    } else {
        None
    };

    let motion_timeout: Duration = Duration::from_secs_f64(config.pause.motion_timeout);
    let mut ready_to_pause_print = false;
    loop {
        match camera.get_state() {
            StateInfo::Streaming => {
                let can_pause = camera.can_pause().await;
                if config.pause.on_disconnect {
                    if let Some(false) = camera.client_connected().await {
                        if can_pause {
                            info!("Pause on disconnect");
                            camera.pause().await.map_err(CameraFailureKind::Retry)?;
                        } else if !ready_to_pause_print {
                            ready_to_pause_print = true;
                            warn!("Not ready to pause");
                        }
                    }
                }
                if config.pause.on_motion {
                    if let Some(motion) = motion.as_mut() {
                        match motion
                            .motion_detected_within(motion_timeout)
                            .with_context(|| "Motion detection unexpectedly stopped")
                            .map_err(CameraFailureKind::Retry)?
                        {
                            Some(false) => {
                                if can_pause {
                                    info!("Pause on motion");
                                    camera.pause().await.map_err(CameraFailureKind::Retry)?;
                                } else if !ready_to_pause_print {
                                    ready_to_pause_print = true;
                                    warn!("Not ready to pause");
                                }
                            }
                            None => {
                                if can_pause {
                                    info!("Pause on motion (start)");
                                    camera.pause().await.map_err(CameraFailureKind::Retry)?;
                                } else if !ready_to_pause_print {
                                    ready_to_pause_print = true;
                                    warn!("Not ready to pause");
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if let Err(e) = camera.is_running().await {
                    return Err(CameraFailureKind::Retry(anyhow!(
                        "Camera has unexpectanely stopped the streaming state: {:?}",
                        e
                    )));
                }
            }
            StateInfo::Paused => {
                if config.pause.on_disconnect {
                    if let Some(true) = camera.client_connected().await {
                        info!("Resume on disconnect");
                        camera.stream().await.map_err(CameraFailureKind::Retry)?;
                    }
                }
                if config.pause.on_motion {
                    if let Some(motion) = motion.as_mut() {
                        if let Some(true) = motion
                            .motion_detected_within(motion_timeout)
                            .with_context(|| "Motion detection unexpectedly stopped")
                            .map_err(CameraFailureKind::Retry)?
                        {
                            info!("Resume on motion");
                            camera.stream().await.map_err(CameraFailureKind::Retry)?;
                        }
                    }
                }
                if let Err(e) = camera.is_running().await {
                    return Err(CameraFailureKind::Retry(anyhow!(
                        "Camera has unexpectanely stopped the paused state: {:?}",
                        e
                    )));
                }
            }
            _ => {
                return Err(CameraFailureKind::Retry(anyhow!(
                    "Camera has unexpectanely stopped the paused state"
                )));
            }
        }
        backoff.spin();
    }
}

fn set_up_tls(config: &Config, rtsp: &RtspServer) {
    let tls_client_auth = match &config.tls_client_auth as &str {
        "request" => TlsAuthenticationMode::Requested,
        "require" => TlsAuthenticationMode::Required,
        "none" => TlsAuthenticationMode::None,
        _ => unreachable!(),
    };
    if let Some(cert_path) = &config.certificate {
        rtsp.set_tls(cert_path, tls_client_auth)
            .expect("Failed to set up TLS");
    }
}

fn set_up_users(users: &[UserConfig], rtsp: &RtspServer) {
    // Setting up users
    let credentials: Vec<_> = users
        .iter()
        .map(|user| (&*user.name, &*user.pass))
        .collect();
    rtsp.set_credentials(&credentials)
        .expect("Failed to set up users");
}

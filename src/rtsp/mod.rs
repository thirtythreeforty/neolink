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
pub(crate) fn main(_opt: Opt, mut config: Config) -> Result<()> {
    let rtsp = Arc::new(RtspServer::new());

    set_up_tls(&config, &rtsp);

    set_up_users(&config.users, &rtsp);

    if config.certificate.is_none() && !config.users.is_empty() {
        warn!(
            "Without a server certificate, usernames and passwords will be exchanged in plaintext!"
        )
    }
    crossbeam::scope(|s| {
        let abort = AbortHandle::new();

        let arc_user_config = Arc::new(config.users);
        for camera_config in config.cameras.drain(..) {
            // Spawn each camera controller in it's own thread
            let user_config = arc_user_config.clone();
            let arc_rtsp = rtsp.clone();
            let abort_handle = abort.clone();
            s.spawn(move |_| {
                let backoff = Backoff::new();

                while abort_handle.is_live() {
                    let failure =
                        camera_main(&camera_config, user_config.as_slice(), arc_rtsp.clone());
                    match failure {
                        Err(CameraFailureKind::Fatal(e)) => {
                            error!("{}: Fatal error: {:?}", camera_config.name, e);
                            break;
                        }
                        Err(CameraFailureKind::Retry(e)) => {
                            warn!("{}: Retryable error: {:?}", camera_config.name, e);
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
        rtsp.run(&config.bind_addr, config.bind_port);
    })
    .map_err(|e| anyhow::anyhow!("Thread panicked: {:?}", e))?;
    Ok(())
}

enum CameraFailureKind {
    Fatal(anyhow::Error),
    Retry(anyhow::Error),
}

fn camera_main(
    config: &CameraConfig,
    user_config: &[UserConfig],
    rtsp: Arc<RtspServer>,
) -> Result<(), CameraFailureKind> {
    // Connect
    let mut camera = RtspCamera::new(config, user_config, rtsp)
        .with_context(|| format!("{}: Could not connect camera", config.name))
        .map_err(CameraFailureKind::Retry)?;
    camera
        .login()
        .with_context(|| format!("{}: Could not login to camera", config.name))
        .map_err(CameraFailureKind::Fatal)?;

    let _ = camera.manage();

    camera
        .stream()
        .with_context(|| format!("{}: Could not start stream", config.name))
        .map_err(CameraFailureKind::Retry)?;

    let backoff = Backoff::new();
    let mut motion = if config.pause.on_motion {
        Some(
            camera
                .motion_data()
                .with_context(|| {
                    "Could not initialise motion detection. Is it supported with this camera?"
                })
                .map_err(CameraFailureKind::Retry)?,
        )
    } else {
        None
    };

    let motion_timeout: Duration = Duration::from_secs_f64(config.pause.motion_timeout);
    loop {
        match camera.get_state() {
            StateInfo::Streaming => {
                let can_pause = camera.can_pause();
                if config.pause.on_disconnect {
                    if let Some(false) = camera.client_connected() {
                        if can_pause {
                            info!("Pause on disconnect");
                            camera.pause().map_err(CameraFailureKind::Retry)?;
                        } else {
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
                                    camera.pause().map_err(CameraFailureKind::Retry)?;
                                } else {
                                    warn!("Not ready to pause");
                                }
                            }
                            None => {
                                if can_pause {
                                    info!("Pause on motion (start)");
                                    camera.pause().map_err(CameraFailureKind::Retry)?;
                                } else {
                                    warn!("Not ready to pause");
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if !camera.is_running() {
                    return Err(CameraFailureKind::Retry(anyhow!(
                        "Camera has unexpectanely stopped the paused state"
                    )));
                }
            }
            StateInfo::Paused => {
                if config.pause.on_disconnect {
                    if let Some(true) = camera.client_connected() {
                        info!("Resume on disconnect");
                        camera.stream().map_err(CameraFailureKind::Retry)?;
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
                            camera.stream().map_err(CameraFailureKind::Retry)?;
                        }
                    }
                }
                if !camera.is_running() {
                    return Err(CameraFailureKind::Retry(anyhow!(
                        "Camera has unexpectanely stopped the paused state"
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

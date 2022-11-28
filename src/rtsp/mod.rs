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
use anyhow::{Context, Result};
use log::*;
use neolink_core::bc_protocol::{BcCamera, Stream};
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

mod abort;
mod cmdline;
mod gst;
mod motion;
mod state;

use super::config::{CameraConfig, Config, UserConfig};
use crate::utils::AddressOrUid;
pub(crate) use cmdline::Opt;
use gst::{GstOutputs, InputMode, PausedSources, RtspServer, TlsAuthenticationMode};
use motion::MotionStream;
use state::States;

/// Entry point for the rtsp subcommand
///
/// Opt is the command line options
pub(crate) fn main(_opt: Opt, config: Config) -> Result<()> {
    let rtsp = &RtspServer::new();

    set_up_tls(&config, rtsp);

    set_up_users(&config.users, rtsp);

    if config.certificate == None && !config.users.is_empty() {
        warn!(
            "Without a server certificate, usernames and passwords will be exchanged in plaintext!"
        )
    }

    crossbeam::scope(|s| {
        for camera in config.cameras {
            if camera.format.is_some() {
                warn!("The format config option of the camera has been removed in favour of auto detection.")
            }
            // Let subthreads share the camera object; in principle I think they could share
            // the object as it sits in the config.cameras block, but I have not figured out the
            // syntax for that.
            let arc_cam = Arc::new(camera);

            let permitted_users =
                get_permitted_users(config.users.as_slice(), &arc_cam.permitted_users);

            let mut stream_outputs: Vec<(Stream, Arc<Mutex<GstOutputs>>)> = vec![];

            // Set up each main and substream according to all the RTSP mount paths we support
            if ["all", "both", "mainStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[
                    &*format!("/{}", arc_cam.name),
                    &*format!("/{}/mainStream", arc_cam.name),
                ];
                let outputs = rtsp
                    .add_stream(paths, &permitted_users)
                    .unwrap();
                stream_outputs.push((Stream::Main, Arc::new(Mutex::new(outputs))));
            }
            if ["all", "both", "subStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[&*format!("/{}/subStream", arc_cam.name)];
                let outputs = rtsp
                    .add_stream(paths, &permitted_users)
                    .unwrap();
                stream_outputs.push((Stream::Sub, Arc::new(Mutex::new(outputs))));
            }
            if ["all", "externStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[&*format!("/{}/externStream", arc_cam.name)];
                let outputs = rtsp
                    .add_stream(paths, &permitted_users)
                    .unwrap();
                stream_outputs.push((Stream::Extern, Arc::new(Mutex::new(outputs))));
            }

            let main_camera = arc_cam.clone();
            s.spawn(move |_| camera_loop(&*main_camera, stream_outputs, true));
        }

        rtsp.run(&config.bind_addr, config.bind_port);
    })
    .unwrap();

    Ok(())
}

fn camera_loop(
    camera_config: &CameraConfig,
    end_points: Vec<(Stream, Arc<Mutex<GstOutputs>>)>,
    manage: bool,
) -> Result<(), anyhow::Error> {
    let min_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(15);
    let mut current_backoff = min_backoff;

    loop {
        let cam_err = camera_main(camera_config, &end_points, manage).unwrap_err();
        for (_, outputs) in end_points.iter() {
            (*outputs.lock().unwrap()).vidsrc.on_stream_error();
            (*outputs.lock().unwrap()).audsrc.on_stream_error();
        }
        // Authentication failures are permanent; we retry everything else
        if cam_err.connected {
            current_backoff = min_backoff;
        }
        if cam_err.login_fail {
            error!(
                "Authentication failed to camera {}, not retrying",
                camera_config.name
            );
            return Err(cam_err.err);
        } else {
            // Should not occur because we don't set the callback up
            // in such a way that it requests graceful shutdown
            return Ok(());
        }
    }
}

struct CameraErr {
    connected: bool,
    login_fail: bool,
    err: anyhow::Error,
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

fn get_permitted_users<'a>(
    users: &'a [UserConfig],
    // not idiomatic as a function argument, but this fn translates the config struct directly:
    permitted_users: &'a Option<Vec<String>>,
) -> HashSet<&'a str> {
    // Helper to build hashset of all users in `users`:
    let all_users_hash = || users.iter().map(|u| u.name.as_str()).collect();

    match permitted_users {
        // If in the camera config there is the user "anyone", or if none is specified but users
        // are defined at all, then we add all users to the camera's allowed list.
        Some(p) if p.iter().any(|u| u == "anyone") => all_users_hash(),
        None if !users.is_empty() => all_users_hash(),

        // The user specified permitted_users
        Some(p) => p.iter().map(String::as_str).collect(),

        // The user didn't specify permitted_users, and there are none defined anyway
        None => ["anonymous"].iter().cloned().collect(),
    }
}

fn camera_main(
    camera_config: &CameraConfig,
    end_points: &[(Stream, Arc<Mutex<GstOutputs>>)],
    manage: bool,
) -> Result<(), CameraErr> {
    let mut connected = false;
    let mut login_fail = false;
    (|| -> Result<(), anyhow::Error> {
        let camera_addr =
            AddressOrUid::new(&camera_config.camera_addr, &camera_config.camera_uid).unwrap();
        let mut camera =
            camera_addr.connect_camera(camera_config.channel_id)
                .with_context(|| {
                    format!(
                        "Failed to connect to camera {} at {} on channel {}",
                        camera_config.name, camera_addr, camera_config.channel_id
                    )
                })?;

        if camera_config.timeout.is_some() {
            warn!("The undocumented `timeout` config option has been removed and is no longer needed.");
            warn!("Please update your config file.");
        }

        info!(
            "{}: Connecting to camera at {}",
            camera_config.name, camera_addr
        );

        info!("{}: Logging in", camera_config.name);
        camera.login(&camera_config.username, camera_config.password.as_deref()).map_err(|e|
            {
                if let neolink_core::Error::AuthFailed = e {
                    login_fail = true;
                }
                e
            }
        ).with_context(|| format!("Failed to login to {}", camera_config.name))?;

        connected = true;
        info!("{}: Connected and logged in", camera_config.name);

        if manage {
            do_camera_management(&mut camera, camera_config).context("Failed to manage the camera settings")?;
        }

        let arc_camera_results: Arc<Mutex<Vec<Result<(), anyhow::Error>>>> = Default::default();
        let arc_camera = Arc::new(camera);

        crossbeam::scope(|s| {
            let arc_abort_handle = Arc::new(AtomicBool::new(false));
            for (stream_name, arc_outputs) in end_points.iter() {
                let stream_display_name = match stream_name {
                    Stream::Main => "Main Stream (Clear)",
                    Stream::Sub => "Sub Stream (Fluent)",
                    Stream::Extern => "Extern Stream (Balanced)",
                };

                info!(
                    "{}: Starting video stream {}",
                    camera_config.name, stream_display_name
                );

                let camera_results = arc_camera_results.clone();
                let camera = arc_camera.clone();
                let outputs = arc_outputs.clone();
                let abort_handle = arc_abort_handle.clone();
                let abort_handle_2 = arc_abort_handle.clone();

                s.spawn(move |_| {
                let camera_result = camera.start_video(&mut *outputs.lock().unwrap(), *stream_name, abort_handle).with_context(|| format!("Error while streaming {}", camera_config.name));
                (*camera_results.lock().unwrap()).push(camera_result);

                let _ = camera.stop_video(*stream_name);
                abort_handle_2.store(true, Ordering::Relaxed);
            });
            }
        }).unwrap();

        let mut camera_results = arc_camera_results.lock().unwrap();
        while camera_results.len() > 0 {
            camera_results.pop().unwrap()?; // Need to take ownership
        }
        unreachable!(); // Should always error before this

    })().map_err(|e| CameraErr{
        connected,
        login_fail,
        err: e,
    })
}

fn do_camera_management(camera: &mut BcCamera, camera_config: &CameraConfig) -> Result<()> {
    let cam_time = camera.get_time()?;
    if let Some(time) = cam_time {
        info!(
            "{}: Camera time is already set: {}",
            camera_config.name, time
        );
    } else {
        use time::OffsetDateTime;
        // We'd like now_local() but it's deprecated - try to get the local time, but if no
        // time zone, fall back to UTC.
        let new_time =
            OffsetDateTime::try_now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());

        warn!(
            "{}: Camera has no time set, setting to {}",
            camera_config.name, new_time
        );
        camera.set_time(new_time)?;
        let cam_time = camera.get_time()?;
        if let Some(time) = cam_time {
            info!("{}: Camera time is now set: {}", camera_config.name, time);
        } else {
            error!(
                "{}: Camera did not accept new time (is {} an admin?)",
                camera_config.name, camera_config.username
            );
        }
    }

    use neolink_core::bc::xml::VersionInfo;
    if let Ok(VersionInfo {
        firmwareVersion: firmware_version,
        ..
    }) = camera.version()
    {
        info!(
            "{}: Camera reports firmware version {}",
            camera_config.name, firmware_version
        );
    } else {
        info!(
            "{}: Could not fetch version information",
            camera_config.name
        );
    }

    Ok(())
}

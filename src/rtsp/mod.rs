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
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use validator::Validate;

mod abort;
mod cmdline;
mod config;
mod gst;
mod motion;
mod state;

use crate::utils::AddressOrUid;
pub(crate) use cmdline::Opt;
use config::{CameraConfig, Config, UserConfig};
use gst::{GstOutputs, RtspServer, TlsAuthenticationMode};
use motion::MotionStream;
use state::States;

/// Entry point for the rtsp subcommand
///
/// Opt is the command line options
pub fn main(opt: Opt) -> Result<()> {
    let config: Config = toml::from_str(
        &fs::read_to_string(&opt.config)
            .with_context(|| format!("Failed to read {:?}", &opt.config))?,
    )
    .with_context(|| format!("Failed to load {:?} as a config file", &opt.config))?;

    config
        .validate()
        .with_context(|| format!("Failed to validate the {:?} config file", &opt.config))?;

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

            // Set up each main and substream according to all the RTSP mount paths we support
            if ["all", "both", "mainStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[
                    &*format!("/{}", arc_cam.name),
                    &*format!("/{}/mainStream", arc_cam.name),
                ];
                let mut outputs = rtsp
                    .add_stream(paths, &permitted_users)
                    .unwrap();
                let main_camera = arc_cam.clone();
                s.spawn(move |_| camera_loop(&*main_camera, Stream::Main, &mut outputs, true));
            }
            if ["all", "both", "subStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[&*format!("/{}/subStream", arc_cam.name)];
                let mut outputs = rtsp
                    .add_stream(paths, &permitted_users)
                    .unwrap();
                let sub_camera = arc_cam.clone();
                let manage = arc_cam.stream == "subStream";
                s.spawn(move |_| camera_loop(&*sub_camera, Stream::Sub, &mut outputs, manage));
            }
            if ["all", "externStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[&*format!("/{}/externStream", arc_cam.name)];
                let mut outputs = rtsp
                    .add_stream(paths, &permitted_users)
                    .unwrap();
                let sub_camera = arc_cam.clone();
                let manage = arc_cam.stream == "externStream";
                s.spawn(move |_| camera_loop(&*sub_camera, Stream::Extern, &mut outputs, manage));
            }
        }

        rtsp.run(&config.bind_addr, config.bind_port);
    })
    .unwrap();

    Ok(())
}

fn camera_loop(
    camera_config: &CameraConfig,
    stream_name: Stream,
    outputs: &mut GstOutputs,
    manage: bool,
) -> Result<(), anyhow::Error> {
    let min_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(15);
    let mut current_backoff = min_backoff;

    loop {
        if let Err(cam_err) = camera_main(camera_config, stream_name, outputs, manage) {
            outputs.vidsrc.on_stream_error();
            outputs.audsrc.on_stream_error();
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
                error!(
                    "Error streaming from camera {}, will retry in {}s: {:?}",
                    camera_config.name,
                    current_backoff.as_secs(),
                    cam_err.err
                )
            }

            std::thread::sleep(current_backoff);
            current_backoff = std::cmp::min(max_backoff, current_backoff * 2);
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
    stream_name: Stream,
    outputs: &mut GstOutputs,
    manage: bool,
) -> Result<(), CameraErr> {
    let mut connected = false;
    let mut login_fail = false;
    (|| {
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

        let stream_display_name = match stream_name {
            Stream::Main => "Main Stream (Clear)",
            Stream::Sub => "Sub Stream (Fluent)",
            Stream::Extern => "Extern Stream (Balanced)",
        };

        info!(
            "{}: Starting video stream {}",
            camera_config.name, stream_display_name
        );

        match &camera_config.motion {
            Some(motion_config) => {
                // Used to get the error from the thread
                let thread_error: Arc<Mutex<Option<anyhow::Error>>> = Default::default();

                // Used to abort all sub threads if any of them request closure
                // and control the motion/client state info
                let state = States::new_from_camera_config(camera_config);
                outputs.set_state(state.clone());

                crossbeam::scope(|s| {
                    let mut motion = MotionStream::new(motion_config.timeout, state.clone());

                    let arc_camera = Arc::new(camera);

                    // Motion detection thread
                    let motion_camera = arc_camera.clone();
                    let motion_camname = camera_config.name.to_string();
                    let motion_error = thread_error.clone();
                    let motion_state = state.clone();
                    s.spawn(move |_| {
                        if let Err(e) = motion_camera.listen_on_motion(&mut motion).with_context(|| format!("Error while reading motion {}", motion_camname)) {
                            // When motion errors set the thread error
                            *motion_error.lock().unwrap() = Some(e);
                        }
                        // Always abort the streams/motion when motion stops regardless of why
                        // so that this thread can reset the camera
                        motion_state.abort();
                        debug!("Motion stopped");
                    });

                    // Stream thread
                    let stream_camera = arc_camera;
                    let stream_camname = camera_config.name.to_string();
                    let stream_error = thread_error.clone();
                    let stream_state = state.clone();
                    s.spawn(move |_| {
                        while stream_state.is_live() {
                            if stream_state.should_stream() || ! outputs.has_last_iframe() {
                                debug!("Stream resumed");
                                // Will block until the rx thread send the abort
                                if let Err(e) = stream_camera.start_video(outputs, stream_name).with_context(|| format!("Error while streaming {}", stream_camname)) {
                                    // When stream errors set the thread error and abort the motion
                                    *stream_error.lock().unwrap() = Some(e);
                                    break;
                                }
                                debug!("Stream paused");
                            } else {
                                // While stream is aborted we send the blank image
                                if let Err(e) = outputs.write_last_iframe().with_context(|| format!("Failed to write no motion image for {}", stream_camname)) {
                                    *stream_error.lock().unwrap() = Some(e);
                                    break;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                        }
                        stream_state.abort();
                        println!("Stream stopped");
                    });
                }).unwrap();

                // Should be the only one holding the thread_error so unwrap it
                // all the way to an actual error
                Err(Arc::try_unwrap(thread_error).unwrap().into_inner().unwrap().unwrap())
            }
            None => {
                // No motion config
                //
                // Just stream always
                camera.start_video(outputs, stream_name).with_context(|| format!("Error while streaming {}", camera_config.name))
            }
        }

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

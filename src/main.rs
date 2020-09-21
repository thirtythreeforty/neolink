use env_logger::Env;
use err_derive::Error;
use gio::TlsAuthenticationMode;
use log::*;
use neolink::bc_protocol::BcCamera;
use neolink::gst::{MaybeAppSrc, RtspServer, StreamFormat};
use neolink::Never;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use validator::Validate;

mod cmdline;
mod config;

use cmdline::Opt;
use config::{CameraConfig, Config, UserConfig};

#[derive(Debug, Error)]
pub enum Error {
    #[error(display = "Configuration parsing error")]
    ConfigError(#[error(source)] toml::de::Error),
    #[error(display = "Communication error")]
    ProtocolError(#[error(source)] neolink::Error),
    #[error(display = "I/O error")]
    IoError(#[error(source)] std::io::Error),
    #[error(display = "Validation error")]
    ValidationError(#[error(source)] validator::ValidationErrors),
}

fn main() -> Result<(), Error> {
    env_logger::from_env(Env::default().default_filter_or("info")).init();

    info!(
        "Neolink {} {}",
        env!("NEOLINK_VERSION"),
        env!("NEOLINK_PROFILE")
    );

    let opt = Opt::from_args();
    let config: Config = toml::from_str(&fs::read_to_string(opt.config)?)?;

    config.validate()?;

    let rtsp = &RtspServer::new();

    set_up_tls(&config, &rtsp);

    set_up_users(&config.users, &rtsp);

    if config.certificate == None && !config.users.is_empty() {
        warn!(
            "Without a server certificate, usernames and passwords will be exchanged in plaintext!"
        )
    }

    crossbeam::scope(|s| {
        for camera in config.cameras {
            let stream_format = match &*camera.format {
                "h264" | "H264" => StreamFormat::H264,
                "h265" | "H265" => StreamFormat::H265,
                custom_format => StreamFormat::Custom(custom_format.to_string()),
            };

            // Let subthreads share the camera object; in principle I think they could share
            // the object as it sits in the config.cameras block, but I have not figured out the
            // syntax for that.
            let arc_cam = Arc::new(camera);

            // The substream always seems to be H264, even on B800 cameras
            let substream_format = match &*arc_cam.format {
                "h264" | "H264" | "h265" | "H265" => StreamFormat::H264,
                custom_format => StreamFormat::Custom(custom_format.to_string()),
            };
            let permitted_users =
                get_permitted_users(config.users.as_slice(), &arc_cam.permitted_users);

            // Set up each main and substream according to all the RTSP mount paths we support
            if ["both", "mainStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[
                    &*format!("/{}", arc_cam.name),
                    &*format!("/{}/mainStream", arc_cam.name),
                ];
                let mut output = rtsp
                    .add_stream(paths, stream_format, &permitted_users)
                    .unwrap();
                let main_camera = arc_cam.clone();
                s.spawn(move |_| camera_loop(&*main_camera, "mainStream", &mut output, true));
            }
            if ["both", "subStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[&*format!("/{}/subStream", arc_cam.name)];
                let mut output = rtsp
                    .add_stream(paths, substream_format, &permitted_users)
                    .unwrap();
                let sub_camera = arc_cam.clone();
                let manage = arc_cam.stream == "subStream";
                s.spawn(move |_| camera_loop(&*sub_camera, "subStream", &mut output, manage));
            }
        }

        rtsp.run(&config.bind_addr, config.bind_port);
    })
    .unwrap();

    Ok(())
}

fn camera_loop(
    camera_config: &CameraConfig,
    stream_name: &str,
    output: &mut MaybeAppSrc,
    manage: bool,
) -> Result<Never, Error> {
    let min_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(15);
    let mut current_backoff = min_backoff;

    loop {
        let cam_err = camera_main(camera_config, stream_name, output, manage).unwrap_err();
        output.on_stream_error();
        // Authentication failures are permanent; we retry everything else
        if cam_err.connected {
            current_backoff = min_backoff;
        }
        match cam_err.err {
            neolink::Error::AuthFailed => {
                error!(
                    "Authentication failed to camera {}, not retrying",
                    camera_config.name
                );
                return Err(cam_err.err.into());
            }
            _ => error!(
                "Error streaming from camera {}, will retry in {}s: {}",
                camera_config.name,
                current_backoff.as_secs(),
                cam_err.err
            ),
        }

        std::thread::sleep(current_backoff);
        current_backoff = std::cmp::min(max_backoff, current_backoff * 2);
    }
}

struct CameraErr {
    connected: bool,
    err: neolink::Error,
}

fn set_up_tls(config: &Config, rtsp: &RtspServer) {
    let tls_client_auth = match &config.tls_client_auth as &str {
        "request" => TlsAuthenticationMode::Requested,
        "require" => TlsAuthenticationMode::Required,
        "none" => TlsAuthenticationMode::None,
        _ => unreachable!(),
    };
    if let Some(cert_path) = &config.certificate {
        rtsp.set_tls(&cert_path, tls_client_auth)
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
    stream_name: &str,
    output: &mut dyn Write,
    manage: bool,
) -> Result<Never, CameraErr> {
    let mut connected = false;
    (|| {
        let mut camera = BcCamera::new_with_addr(camera_config.camera_addr)?;
        if camera_config.timeout.is_some() {
            warn!("The undocumented `timeout` config option has been removed and is no longer needed.");
            warn!("Please update your config file.");
        }

        info!(
            "{}: Connecting to camera at {}",
            camera_config.name, camera_config.camera_addr
        );
        camera.connect()?;

        camera.login(&camera_config.username, camera_config.password.as_deref())?;

        connected = true;
        info!("{}: Connected and logged in", camera_config.name);

        if manage {
            let cam_time = camera.get_time()?;
            if let Some(time) = cam_time {
                info!(
                    "{}: Camera time is already set: {}",
                    camera_config.name, time
                );
            } else {
                let new_time = time::OffsetDateTime::now_local();
                warn!(
                    "{}: Camera has no time set, setting to {}",
                    camera_config.name, new_time
                );
                camera.set_time(new_time)?;
                let cam_time = camera.get_time()?;
                if let Some(time) = cam_time {
                    info!(
                        "{}: Camera time is now set: {}",
                        camera_config.name, time
                    );
                } else {
                    error!("{}: Camera did not accept new time (is {} an admin?)", camera_config.name, camera_config.username);
                }
            }
        }

        info!(
            "{}: Starting video stream {}",
            camera_config.name, stream_name
        );
        camera.start_video(output, stream_name, camera_config.channel_id)
    })()
    .map_err(|err| CameraErr { connected, err })
}

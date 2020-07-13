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

    crossbeam::scope(|s| {
        for camera in config.cameras {
            let stream_format = match &*camera.format {
                "h264" | "H264" => StreamFormat::H264,
                "h265" | "H265" => StreamFormat::H265,
                custom_format @ _ => StreamFormat::Custom(custom_format.to_string()),
            };

            // The substream always seems to be H264, even on B800 cameras
            let substream_format = match &*camera.format {
                "h264" | "H264" | "h265" | "H265" => StreamFormat::H264,
                custom_format @ _ => StreamFormat::Custom(custom_format.to_string()),
            };
            let permitted_user = get_permitted_users(&config.users, &camera.permitted_users);

            // Let subthreads share the camera object; in principle I think they could share
            // the object as it sits in the config.cameras block, but I have not figured out the
            // syntax for that.
            let arc_cam = Arc::new(camera);

            // Set up each main and substream according to all the RTSP mount paths we support
            if ["both", "mainStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[
                    &*format!("/{}", arc_cam.name),
                    &*format!("/{}/mainStream", arc_cam.name),
                ];
                let mut output = rtsp
                    .add_stream(paths, &stream_format, &permitted_user)
                    .unwrap();
                let main_camera = arc_cam.clone();
                s.spawn(move |_| camera_loop(&*main_camera, "mainStream", &mut output));
            }
            if ["both", "subStream"].iter().any(|&e| e == arc_cam.stream) {
                let paths = &[&*format!("/{}/subStream", arc_cam.name)];
                let mut output = rtsp
                    .add_stream(paths, &substream_format, &permitted_user)
                    .unwrap();
                let sub_camera = arc_cam.clone();
                s.spawn(move |_| camera_loop(&*sub_camera, "subStream", &mut output));
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
) -> Result<Never, Error> {
    let min_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(15);
    let mut current_backoff = min_backoff;

    loop {
        let cam_err = camera_main(camera_config, stream_name, output).unwrap_err();
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

fn set_up_users(users: &Vec<UserConfig>, rtsp: &RtspServer) {
    // Setting up users
    let mut credentials = vec![];
    for user in users {
        let name = &user.name;
        let pass = &user.pass;
        let user_pass = match (name, pass) {
            (Some(name), Some(pass)) => Some((&name as &str, &pass as &str)),
            (Some(_), None) | (None, Some(_)) => {
                warn!("Username and password must be supplied together - ignoring [[users]] entry");
                None
            }
            _ => None,
        };
        credentials.push(user_pass);
    }
    rtsp.set_credentials(&credentials)
        .expect("Failed to set up users.");
}

fn get_permitted_users(
    users: &Vec<UserConfig>,
    current_permitted_users: &Vec<String>,
) -> Vec<String> {
    // This is required to handle the special case of "anyone"
    // ===Special set up of "anyone"===
    // If in the camera config there is the user "anyone"
    // Then we add all users to the cameras config. including unauth
    let mut new_permitted_users = HashSet::new();
    if current_permitted_users.contains(&"anyone".to_string()) {
        for credentials in users {
            if let Some(user) = &credentials.name {
                new_permitted_users.insert(user.to_string());
            }
        }
    } else {
        for user in current_permitted_users {
            new_permitted_users.insert(user.to_string());
        }
    }
    let mut result = vec![];
    for user in new_permitted_users {
        result.push(user);
    }
    result
}

fn camera_main(
    camera_config: &CameraConfig,
    stream_name: &str,
    output: &mut dyn Write,
) -> Result<Never, CameraErr> {
    let mut connected = false;
    (|| {
        let mut camera = BcCamera::new_with_addr(camera_config.camera_addr)?;
        if let Some(timeout) = camera_config.timeout {
            camera.set_rx_timeout(timeout);
        }

        info!(
            "{}: Connecting to camera at {}",
            camera_config.name, camera_config.camera_addr
        );
        camera.connect()?;

        camera.login(&camera_config.username, camera_config.password.as_deref())?;

        connected = true;

        info!(
            "{}: Connected to camera, starting video stream {}",
            camera_config.name, stream_name
        );
        camera.start_video(output, stream_name)
    })()
    .map_err(|err| CameraErr { connected, err })
}

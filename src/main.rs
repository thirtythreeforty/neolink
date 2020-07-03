#[macro_use] extern crate validator_derive;
#[macro_use] extern crate lazy_static;

use env_logger::Env;
use err_derive::Error;
use log::*;
use neolink::bc_protocol::BcCamera;
use neolink::gst::{MaybeAppSrc, RtspServer, StreamFormat};
use neolink::Never;
use std::fs;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use validator::Validate;
use gio::TlsAuthenticationMode;

mod cmdline;
mod config;

use cmdline::Opt;
use config::{CameraConfig, Config};

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

    match config.validate() {
        Ok(_) => (),
        Err(e) => return Err(Error::ValidationError(e)),
    };

    let rtsp = &RtspServer::new();

    let tls_client_auth = match &config.tls_client_auth as &str {
        "request" => TlsAuthenticationMode::Requested,
        "require" => TlsAuthenticationMode::Required,
        "none" => TlsAuthenticationMode::None,
        _ => unreachable!(),
    };
    rtsp.set_tls(&config.certificate, tls_client_auth).expect("Failed to set up TLS");

    let name = &config.username;
    let pass = &config.password;
    let user_pass = match (name, pass)  {
        (Some(name), Some(pass)) => Some((&name as &str, &pass as &str)),
        _ => None,
    };
    rtsp.set_credentials(user_pass).expect("Failed to set up users.");


    crossbeam::scope(|s| {
        for camera in config.cameras {
            let stream_format = match &*camera.format {
                "h264"|"H264" => StreamFormat::H264,
                "h265"|"H265" => StreamFormat::H265,
                custom_format @ _ => StreamFormat::Custom(custom_format.to_string())
            };

            // Let subthreads share the camera object; in principle I think they could share
            // the object as it sits in the config.cameras block, but I have not figured out the
            // syntax for that.
            let arc_cam = Arc::new(camera);

            // Set up each main and substream according to all the RTSP mount paths we support
            if arc_cam.stream == "both" || arc_cam.stream == "mainStream" {
                let paths = &[
                    &arc_cam.name,
                    &*format!("{}/mainStream", arc_cam.name),
                ];
                let mut output = rtsp.add_stream(paths, &stream_format).unwrap();
                let main_camera = arc_cam.clone();
                s.spawn(move |_| camera_loop(&*main_camera, &mut output));
            }
            if arc_cam.stream == "both" || arc_cam.stream == "subStream" {
                let paths = &[&*format!("{}/subStream", arc_cam.name)];
                let mut output = rtsp.add_stream(paths, &stream_format).unwrap();
                let sub_camera = arc_cam.clone();
                s.spawn(move |_| camera_loop(&*sub_camera, &mut output));
            }
        }

        rtsp.run(&config.bind_addr, config.bind_port);
    })
    .unwrap();

    Ok(())
}

fn camera_loop(camera_config: &CameraConfig, output: &mut MaybeAppSrc) -> Result<Never, Error> {
    let min_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(15);
    let mut current_backoff = min_backoff;

    loop {
        let cam_err = camera_main(camera_config, output).unwrap_err();
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

fn camera_main(camera_config: &CameraConfig, output: &mut dyn Write) -> Result<Never, CameraErr> {
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
            camera_config.name, camera_config.stream
        );
        camera.start_video(output, &camera_config.stream)
    })()
    .map_err(|err| CameraErr { connected, err })
}

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
use std::time::Duration;
use structopt::StructOpt;
use validator::Validate;

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
    let mut config: Config = toml::from_str(&fs::read_to_string(opt.config)?)?;

    match config.validate() {
        Ok(_) => (),
        Err(e) => return Err(Error::ValidationError(e)),
    };
    for camera in &config.cameras {
        match camera.validate() {
            Ok(_) => (),
            Err(e) => return Err(Error::ValidationError(e)),
        };
    }

    // Setup auto sub streams, we do this by looping the cameras
    // If the stream type is both we clone the config
    // On one of the clones we set stream_source to "mainStream" on the other "subStream"
    // We also change the mount name by appending "/mainStream" or "/subStream"
    // On the original uncloned config we change the stream from "both" to "mainStream" and leave the mount name unchanged
    let mut new_cam_configs = vec![];

    for camera_config in &mut config.cameras {
        if camera_config.stream == "both" {
            let mut main_camera_config = camera_config.clone();
            let mut sub_camera_config = camera_config.clone();

            camera_config.stream = "mainStream".to_string();

            main_camera_config.stream = "mainStream".to_string();
            main_camera_config.name = format!("{}/{}", main_camera_config.name, main_camera_config.stream);
            new_cam_configs.push(main_camera_config);

            sub_camera_config.stream = "subStream".to_string();
            sub_camera_config.name = format!("{}/{}", sub_camera_config.name, sub_camera_config.stream);
            sub_camera_config.format = "h264".to_string(); // Assuming always H264 on subStream: TODO: Autodetect
            new_cam_configs.push(sub_camera_config);
        }
    }
    config.cameras.append(&mut new_cam_configs);

    let rtsp = &RtspServer::new();

    crossbeam::scope(|s| {
        for camera in config.cameras {
            s.spawn(move |_| {
                // TODO handle these errors
                let cam_format :&str = &camera.format;
                let stream_format = match cam_format {
                    "h264"|"H264" => StreamFormat::H264,
                    "h265"|"H265" => StreamFormat::H265,
                    custom_format @ _ => StreamFormat::Custom(custom_format.to_string())
                };
                let mut output = rtsp.add_stream(&camera.name, &stream_format).unwrap();
                camera_loop(&camera, &mut output)
            });
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

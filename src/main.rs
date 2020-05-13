#![allow(dead_code)]
#![allow(unused_variables)]
mod bc;
mod bc_protocol;
mod config;
mod cmdline;
mod gst;

use bc_protocol::BcCamera;
use config::{Config, CameraConfig};
use cmdline::Opt;
use err_derive::Error;
use gst::RtspServer;
use log::*;
use std::fs;
use std::time::Duration;
use std::io::Write;
use structopt::StructOpt;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display="Configuration parsing error")]
    ConfigError(#[error(source)] toml::de::Error),
    #[error(display="Communication error")]
    ProtocolError(#[error(source)] bc_protocol::Error),
    #[error(display="I/O error")]
    IoError(#[error(source)] std::io::Error),
}

fn main() -> Result<(), Error> {
    env_logger::init();
    let opt = Opt::from_args();
    let config: Config = toml::from_str(&fs::read_to_string(opt.config)?)?;

    let rtsp = &RtspServer::new();

    crossbeam::scope(|s| {
        for camera in config.cameras {
            s.spawn(move |_| {
                // TODO handle these errors
                let mut output = rtsp.add_stream(&camera.name).unwrap(); // TODO
                camera_loop(&camera, &mut output)
            });
        }

        rtsp.run(&config.bind_addr);
    }).unwrap();

    Ok(())
}

fn camera_loop(camera_config: &CameraConfig, output: &mut dyn Write) -> Result<(), Error> {
    let mut current_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(15);

    loop {
        match camera_main(camera_config, output) {
            // Authentication failures are permanent; we retry everything else
            Err(err @ bc_protocol::Error::AuthFailed) => {
                error!("Authentication failed to camera {}, not retrying", camera_config.name);
                return Err(err.into());
            }
            Ok(_) => error!("Camera {} stream stopped unexpectedly, will retry in {}s", camera_config.name, current_backoff.as_secs()),
            Err(err) => error!("Error streaming from camera {}, will retry in {}s: {}", camera_config.name, current_backoff.as_secs(), err),
        }

        std::thread::sleep(current_backoff);
        current_backoff = std::cmp::min(max_backoff, current_backoff * 2);
	}
}

fn camera_main(camera_config: &CameraConfig, output: &mut dyn Write) -> Result<(), bc_protocol::Error> {
    let mut camera = BcCamera::new_with_addr(camera_config.camera_addr)?;
    if let Some(timeout) = camera_config.timeout {
        camera.set_rx_timeout(timeout);
    }

    println!("{}: Connecting to camera at {}", camera_config.name, camera_config.camera_addr);
    camera.connect()?;

    camera.login(&camera_config.username, camera_config.password.as_deref())?;

    println!("{}: Connected to camera, starting video stream", camera_config.name);
    camera.start_video(output)?;

    unreachable!()
}

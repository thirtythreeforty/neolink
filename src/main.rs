use neolink::bc_protocol::BcCamera;
use neolink::gst::RtspServer;
use err_derive::Error;
use log::*;
use std::fs;
use std::time::Duration;
use std::io::Write;
use structopt::StructOpt;

mod cmdline;
mod config;

use config::{Config, CameraConfig};
use cmdline::Opt;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display="Configuration parsing error")]
    ConfigError(#[error(source)] toml::de::Error),
    #[error(display="Communication error")]
    ProtocolError(#[error(source)] neolink::Error),
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
    let min_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(15);
    let mut current_backoff = min_backoff;

    loop {
        match camera_main(camera_config, output) {
            // Authentication failures are permanent; we retry everything else
            Ok(_) => error!("Camera {} stream stopped unexpectedly, will retry in {}s",
                            camera_config.name, current_backoff.as_secs()),
            Err(cam_err) => {
                if cam_err.connected {
                    current_backoff = min_backoff;
                }
                match cam_err.err {
                    neolink::Error::AuthFailed => {
                        error!("Authentication failed to camera {}, not retrying", camera_config.name);
                        return Err(cam_err.err.into());
                    }
                    _ => error!("Error streaming from camera {}, will retry in {}s: {}",
                                camera_config.name, current_backoff.as_secs(), cam_err.err),
                }
            }
        }

        std::thread::sleep(current_backoff);
        current_backoff = std::cmp::min(max_backoff, current_backoff * 2);
	}
}

fn camera_main(camera_config: &CameraConfig, output: &mut dyn Write) -> Result<(), CameraErr> {
    let mut camera = BcCamera::new_with_addr(camera_config.camera_addr).map_err(CameraErr::before_connect)?;
    if let Some(timeout) = camera_config.timeout {
        camera.set_rx_timeout(timeout);
    }

    println!("{}: Connecting to camera at {}", camera_config.name, camera_config.camera_addr);
    camera.connect().map_err(CameraErr::before_connect)?;

    camera.login(&camera_config.username, camera_config.password.as_deref()).map_err(CameraErr::after_connect)?;

    println!("{}: Connected to camera, starting video stream", camera_config.name);
    camera.start_video(output).map_err(CameraErr::after_connect)?;

    unreachable!()
}

struct CameraErr {
    connected: bool,
    err: neolink::Error,
}

impl CameraErr {
    fn before_connect<E: Into<neolink::Error>>(e: E) -> Self {
        CameraErr { connected: false, err: e.into() }
    }
    fn after_connect<E: Into<neolink::Error>>(e: E) -> Self {
        CameraErr { connected: true, err: e.into() }
    }
}

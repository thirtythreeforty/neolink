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
use crossbeam_utils::thread;
use err_derive::Error;
use std::fs;
use std::net::TcpListener;
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
    let opt = Opt::from_args();
    let config: Config = toml::from_str(&fs::read_to_string(opt.config)?)?;

    thread::scope(|s| {
        for camera in config.cameras {
            s.spawn(move |_| {
                // TODO handle these errors
                camera_main(&camera)
            });
        }
    }).unwrap();

    Ok(())
}

fn camera_main(camera_config: &CameraConfig) -> Result<(), Error> {
    let mut camera = BcCamera::new_with_addr(camera_config.camera_addr)?;

    println!("{}: Connecting to camera at {}", camera_config.name, camera_config.camera_addr);

    camera.connect()?;
    camera.login(&camera_config.username, camera_config.password.as_deref())?;

    let bind_addr = &camera_config.bind_addr;
    println!("{}: Logged in to camera; awaiting connection on {}", camera_config.name, bind_addr);

    let listener = TcpListener::bind(bind_addr)?;
    let (mut out_socket, remote_addr) = listener.accept()?;

    println!("{}: Connected to {}, starting video stream", camera_config.name, remote_addr);

    camera.start_video(&mut out_socket)?;

    Ok(())
}

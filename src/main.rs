#![allow(dead_code)] // while I'm still fleshing this out

use err_derive::Error;
use std::env::args;
use std::net::TcpListener;

mod bc;
mod bc_protocol;

use bc_protocol::BcCamera;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display="Communication error")]
    ProtocolError(#[error(source)] bc_protocol::Error),
    #[error(display="Socket setup error")]
    IoError(#[error(source)] std::io::Error),
}

fn main() -> Result<(), Error> {
    let mut camera = BcCamera::new_with_addr(args().nth(1).unwrap())?;
    camera.connect()?;
    camera.login("admin", Some("12345678"))?;

    let bind_addr = "0.0.0.0:9999";
    println!("Logged in to camera; awaiting connection on {}", bind_addr);

    let listener = TcpListener::bind(bind_addr)?;
    let (mut out_socket, remote_addr) = listener.accept()?;

    println!("Connected to {}, starting video stream", remote_addr);

    camera.start_video(&mut out_socket)?;

    Ok(())
}

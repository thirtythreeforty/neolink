#![warn(missing_docs)]
//!
//! # Neolink
//!
//! Neolink is a small program that acts a general contol interface for Reolink IP cameras.
//!
//! It contains sub commands for running an rtsp proxy which can be used on Reolink cameras
//! that do not nativly support RTSP.
//!
use env_logger::Env;
use log::*;
use structopt::StructOpt;

mod cmdline;
mod errors;
mod rtsp;

use cmdline::{Command, Opt};
use errors::Error;

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!(
        "Neolink {} {}",
        env!("NEOLINK_VERSION"),
        env!("NEOLINK_PROFILE")
    );

    let opt = Opt::from_args();

    match opt.cmd {
        Command::Rtsp(opts) => {
            rtsp::main(opts)?;
        }
    }

    Ok(())
}

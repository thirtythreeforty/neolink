#![warn(missing_docs)]
//!
//! # Neolink
//!
//! Neolink is a small program that acts a general contol interface for Reolink IP cameras.
//!
//! It contains sub commands for running an rtsp proxy which can be used on Reolink cameras
//! that do not nativly support RTSP.
//!
use anyhow::Result;
use env_logger::Env;
use log::*;
use structopt::StructOpt;

mod cmdline;
mod reboot;
mod rtsp;
mod statusled;
mod talk;
mod utils;

use cmdline::{Command, Opt};

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!(
        "Neolink {} {}",
        env!("NEOLINK_VERSION"),
        env!("NEOLINK_PROFILE")
    );

    let opt = Opt::from_args();

    match (opt.cmd, opt.config) {
        (None, None) => {
            // Should be caught at the clap validation
            unreachable!();
        }
        (None, Some(config)) => {
            warn!(
                "Deprecated command line option. Please use: `neolink rtsp --config={:?}`",
                config
            );
            rtsp::main(rtsp::Opt { config })?;
        }
        (Some(_), Some(_)) => error!("--config should be given after the subcommand"),
        (Some(Command::Rtsp(opts)), None) => {
            rtsp::main(opts)?;
        }
        (Some(Command::StatusLight(opts)), None) => {
            statusled::main(opts)?;
        }
        (Some(Command::Reboot(opts)), None) => {
            reboot::main(opts)?;
        }
        (Some(Command::Talk(opts)), None) => {
            talk::main(opts)?;
        }
    }

    Ok(())
}

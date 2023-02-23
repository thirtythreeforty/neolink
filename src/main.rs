#![warn(missing_docs)]
//!
//! # Neolink
//!
//! Neolink is a small program that acts a general contol interface for Reolink IP cameras.
//!
//! It contains sub commands for running an rtsp proxy which can be used on Reolink cameras
//! that do not nativly support RTSP.
//!
use anyhow::{Context, Result};
use env_logger::Env;
use log::*;
use std::fs;
use structopt::StructOpt;
use validator::Validate;

mod cmdline;
mod config;
mod image;
mod mqtt;
mod pir;
mod reboot;
mod rtsp;
mod statusled;
mod talk;
mod utils;

use cmdline::{Command, Opt};
use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!(
        "Neolink {} {}",
        env!("NEOLINK_VERSION"),
        env!("NEOLINK_PROFILE")
    );

    let opt = Opt::from_args();

    let conf_path = opt.config.context("Must supply --config file")?;
    let config: Config = toml::from_str(
        &fs::read_to_string(&conf_path)
            .with_context(|| format!("Failed to read {:?}", conf_path))?,
    )
    .with_context(|| format!("Failed to parse the {:?} config file", conf_path))?;

    config
        .validate()
        .with_context(|| format!("Failed to validate the {:?} config file", conf_path))?;

    match opt.cmd {
        None => {
            warn!(
                "Deprecated command line option. Please use: `neolink rtsp --config={:?}`",
                config
            );
            rtsp::main(rtsp::Opt {}, config).await?;
        }
        Some(Command::Rtsp(opts)) => {
            rtsp::main(opts, config).await?;
        }
        Some(Command::StatusLight(opts)) => {
            statusled::main(opts, config).await?;
        }
        Some(Command::Reboot(opts)) => {
            reboot::main(opts, config).await?;
        }
        Some(Command::Pir(opts)) => {
            pir::main(opts, config).await?;
        }
        Some(Command::Talk(opts)) => {
            talk::main(opts, config).await?;
        }
        Some(Command::Mqtt(opts)) => {
            mqtt::main(opts, config).await?;
        }
        Some(Command::Image(opts)) => {
            image::main(opts, config).await?;
        }
    }

    Ok(())
}

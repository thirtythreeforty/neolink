#![warn(unused_crate_dependencies)]
#![warn(missing_docs)]
#![warn(clippy::todo)]
//!
//! # Neolink
//!
//! Neolink is a small program that acts a general contol interface for Reolink IP cameras.
//!
//! It contains sub commands for running an rtsp proxy which can be used on Reolink cameras
//! that do not nativly support RTSP.
//!
//! This program is free software: you can redistribute it and/or modify it under the terms of the
//! GNU General Public License as published by the Free Software Foundation, either version 3 of
//! the License, or (at your option) any later version.
//!
//! This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
//! without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See
//! the GNU General Public License for more details.
//!
//! You should have received a copy of the GNU General Public License along with this program. If
//! not, see <https://www.gnu.org/licenses/>.
//!
//! Neolink source code is available online at <https://github.com/thirtythreeforty/neolink>
//!
#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

use anyhow::{Context, Result};
use clap::Parser;
use env_logger::Env;
use log::*;
use std::fs;
use validator::Validate;

mod battery;
mod cmdline;
mod common;
mod config;
mod image;
mod mqtt;
mod pir;
mod ptz;
mod reboot;
mod rtsp;
mod statusled;
mod talk;
mod utils;

use cmdline::{Command, Opt};
use common::NeoReactor;
use config::Config;
use console_subscriber as _;

pub(crate) type AnyResult<T> = Result<T, anyhow::Error>;

#[cfg(tokio_unstable)]
fn tokio_console_enable() {
    info!("Tokio Console Enabled");
    console_subscriber::init();
}

#[cfg(not(tokio_unstable))]
fn tokio_console_enable() {
    debug!("Tokio Console Disabled");
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!(
        "Neolink {} {}",
        env!("NEOLINK_VERSION"),
        env!("NEOLINK_PROFILE")
    );

    let opt = Opt::parse();

    let conf_path = opt.config.context("Must supply --config file")?;
    let config: Config = toml::from_str(
        &fs::read_to_string(&conf_path)
            .with_context(|| format!("Failed to read {:?}", conf_path))?,
    )
    .with_context(|| format!("Failed to parse the {:?} config file", conf_path))?;

    config
        .validate()
        .with_context(|| format!("Failed to validate the {:?} config file", conf_path))?;

    if config.tokio_console {
        tokio_console_enable();
    }

    let neo_reactor = NeoReactor::new(config.clone()).await;

    match opt.cmd {
        None => {
            warn!(
                "Deprecated command line option. Please use: `neolink rtsp --config={:?}`",
                config
            );
            rtsp::main(rtsp::Opt {}, neo_reactor.clone()).await?;
        }
        Some(Command::Rtsp(opts)) => {
            rtsp::main(opts, neo_reactor.clone()).await?;
        }
        Some(Command::StatusLight(opts)) => {
            statusled::main(opts, neo_reactor.clone()).await?;
        }
        Some(Command::Reboot(opts)) => {
            reboot::main(opts, neo_reactor.clone()).await?;
        }
        Some(Command::Pir(opts)) => {
            pir::main(opts, neo_reactor.clone()).await?;
        }
        Some(Command::Ptz(opts)) => {
            ptz::main(opts, neo_reactor.clone()).await?;
        }
        Some(Command::Talk(opts)) => {
            talk::main(opts, neo_reactor.clone()).await?;
        }
        Some(Command::Mqtt(opts)) => {
            mqtt::main(opts, neo_reactor.clone()).await?;
        }
        Some(Command::MqttRtsp(opts)) => {
            tokio::select! {
                v = mqtt::main(opts, neo_reactor.clone()) => v,
                v = rtsp::main(rtsp::Opt {}, neo_reactor.clone()) => v,
            }?;
        }
        Some(Command::Image(opts)) => {
            image::main(opts, neo_reactor.clone()).await?;
        }
        Some(Command::Battery(opts)) => {
            battery::main(opts, neo_reactor.clone()).await?;
        }
    }

    Ok(())
}

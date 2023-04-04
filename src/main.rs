#![warn(missing_docs)]
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
use anyhow::{Context, Result};
use env_logger::Env;
use log::*;
use std::fs;
use structopt::StructOpt;
use validator::Validate;

mod cmdline;
mod config;
mod pir;
mod readpir;
mod reboot;
mod rtsp;
mod statusled;
mod talk;
mod utils;

use cmdline::{Command, Opt};
use config::Config;

fn main() -> Option<u8> {
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
            rtsp::main(rtsp::Opt {}, config)?;
        }
        Some(Command::Rtsp(opts)) => {
            rtsp::main(opts, config)?;
        }
        Some(Command::StatusLight(opts)) => {
            statusled::main(opts, config)?;
        }
        Some(Command::Reboot(opts)) => {
            reboot::main(opts, config)?;
        }
        Some(Command::Pir(opts)) => {
            pir::main(opts, config)?;
        }
        Some(Command::ReadPir(opts)) => {
            return Some(readpir::main(opts, config)?);
        }
        Some(Command::Talk(opts)) => {
            talk::main(opts, config)?;
        }
    }

    None
}

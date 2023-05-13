///
/// # Neolink PTZ Control
///
/// This module handles the controls of the PTZ commands
///
/// # Usage
///
/// ```bash
/// # Rotate left for 300 milliseconds
/// neolink status-light --config=config.toml CameraName control 300 left
/// # Print the list of preset positions
/// neolink status-light --config=config.toml CameraName preset
/// # Move the camera to preset ID 0
/// neolink status-light --config=config.toml CameraName preset 0
/// # Save the current position as preset ID 0 with name PresetName
/// neolink status-light --config=config.toml CameraName preset 0 PresetName
/// ```
///
use anyhow::{Context, Result};
use std::thread::sleep;
use std::time::Duration;

mod cmdline;

use super::config::Config;
use crate::utils::find_and_connect;
use crate::ptz::cmdline::PtzCommand;
pub(crate) use cmdline::Opt;

/// Entry point for the ptz subcommand
///
/// Opt is the command line options
pub(crate) fn main(opt: Opt, config: Config) -> Result<()> {
    let camera = find_and_connect(&config, &opt.camera)?;

    match opt.cmd {
        PtzCommand::Preset {preset_id, name} => {
            if preset_id.is_some() {
                camera
                    .set_ptz_preset(preset_id.unwrap(), name)
                    .context("Unable to set PTZ preset")?;
            } else {
                let preset_list = camera
                    .get_ptz_preset()
                    .context("Unable to get PTZ presets")?;
                println!("Available presets:\nID Name");
                for preset in preset_list.preset_list.unwrap().preset {
                    println!("{:<2} {}", preset.id, preset.name.unwrap());
                }
            }
        },
        PtzCommand::Control {duration, command} => {
            camera
                .ptz_control(32, command)
                .context("Unable to execute PTZ move command")?;
            sleep(Duration::from_millis(duration as u64));
            camera
                .ptz_control(0, "stop".parse()?)
                .context("Unable to execute PTZ move command")?;
        }
    };
    Ok(())
}

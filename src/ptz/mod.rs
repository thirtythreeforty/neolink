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
use crate::ptz::cmdline::CmdDirection;
use crate::ptz::cmdline::PtzCommand;
use crate::utils::find_and_connect;
pub(crate) use cmdline::Opt;
use neolink_core::bc_protocol::Direction;

/// Entry point for the ptz subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config) -> Result<()> {
    let camera = find_and_connect(&config, &opt.camera).await?;

    match opt.cmd {
        PtzCommand::Preset { preset_id, name } => {
            if preset_id.is_some() {
                camera
                    .set_ptz_preset(preset_id.unwrap(), name)
                    .await
                    .context("Unable to set PTZ preset")
                    .expect("TODO: panic message");
            } else {
                let preset_list = camera
                    .get_ptz_preset()
                    .await
                    .context("Unable to get PTZ presets")?;
                println!("Available presets:\nID Name");
                for preset in preset_list.preset_list.unwrap().preset {
                    println!("{:<2} {}", preset.id, preset.name.unwrap());
                }
            }
        }
        PtzCommand::Control { duration, command } => {
            let direction = match command {
                CmdDirection::Left => Direction::Left,
                CmdDirection::Right => Direction::Right,
                CmdDirection::Up => Direction::Up,
                CmdDirection::Down => Direction::Down,
                CmdDirection::In => Direction::In,
                CmdDirection::Out => Direction::Out,
                CmdDirection::Stop => Direction::Stop,
            };
            camera
                .send_ptz(direction, 32_f32)
                .await
                .context("Unable to execute PTZ move command")?;
            sleep(Duration::from_millis(duration as u64));
            camera
                .send_ptz(Direction::Stop, 0_f32)
                .await
                .context("Unable to execute PTZ move command")?;
        }
    };
    Ok(())
}

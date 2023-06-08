///
/// # Neolink PTZ Control
///
/// This module handles the controls of the PTZ commands
///
/// # Usage
///
/// ```bash
/// # Rotate left by 32
/// neolink ptz --config=config.toml CameraName control 32 left
/// # Rotate left by 32 at speed 10 (speed not supported on most camera)
/// neolink ptz --config=config.toml CameraName control 32 left 10
/// # Print the list of preset positions
/// neolink ptz --config=config.toml CameraName preset
/// # Move the camera to preset ID 0
/// neolink ptz --config=config.toml CameraName preset 0
/// # Save the current position as preset ID 0 with name PresetName
/// neolink ptz --config=config.toml CameraName assign 0 PresetName
/// ```
///
use anyhow::{Context, Result};
use tokio::time::{sleep, Duration};

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
        PtzCommand::Preset { preset_id } => {
            if let Some(preset_id) = preset_id {
                camera
                    .moveto_ptz_preset(preset_id)
                    .await
                    .context("Unable to set PTZ preset")
                    .expect("TODO: panic message");
            } else {
                let preset_list = camera
                    .get_ptz_preset()
                    .await
                    .context("Unable to get PTZ presets")?;
                println!("Available presets:\nID Name");
                for preset in preset_list.preset_list.preset {
                    println!("{:<2} {:?}", preset.id, preset.name);
                }
            }
        }
        PtzCommand::Assign { preset_id, name } => {
            camera
                .set_ptz_preset(preset_id, name)
                .await
                .context("Unable to set PTZ preset")
                .expect("TODO: panic message");
        }
        PtzCommand::Control {
            amount,
            command,
            speed,
        } => {
            let direction = match command {
                CmdDirection::Left => Direction::Left,
                CmdDirection::Right => Direction::Right,
                CmdDirection::Up => Direction::Up,
                CmdDirection::Down => Direction::Down,
                CmdDirection::In => Direction::In,
                CmdDirection::Out => Direction::Out,
                CmdDirection::Stop => Direction::Stop,
            };
            let speed = speed.unwrap_or(32) as f32;
            let seconds = amount as f32 / speed;
            let duration = Duration::from_secs_f32(seconds);
            camera
                .send_ptz(direction, speed)
                .await
                .context("Unable to execute PTZ move command")?;
            sleep(duration).await;
            camera
                .send_ptz(Direction::Stop, 0_f32)
                .await
                .context("Unable to execute PTZ move command")?;
        }
    };
    Ok(())
}

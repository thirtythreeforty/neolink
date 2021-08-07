///
/// # Neolink Talk
///
/// This module can be used to send adpcm data for the camera to play
///
/// The adpcm data needs to be in DVI-4 layout
///
/// # Usage
///
/// ```bash
/// neolink status-light --config=config.toml --adpcm-file=data.adpcm --sample-rate=16000 --block-size=512 CameraName
/// ```
///
use log::*;
use neolink_core::{bc::xml::TalkConfig, bc_protocol::BcCamera};
use std::fs;
use validator::Validate;

mod cmdline;
mod config;
mod errors;
mod gst;

pub(crate) use cmdline::Opt;
use config::Config;
pub(crate) use errors::Error;

/// Entry point for the talk subcommand
///
/// Opt is the command line options
pub fn main(opt: Opt) -> Result<(), Error> {
    let config: Config = toml::from_str(&fs::read_to_string(opt.config)?)?;

    config.validate()?;

    let mut cam_found = false;
    for camera_config in &config.cameras {
        if opt.camera == camera_config.name {
            cam_found = true;
            info!(
                "{}: Connecting to camera at {}",
                camera_config.name, camera_config.camera_addr
            );

            let mut camera =
                BcCamera::new_with_addr(&camera_config.camera_addr, camera_config.channel_id)?;

            info!("{}: Logging in", camera_config.name);
            camera.login(&camera_config.username, camera_config.password.as_deref())?;

            info!("{}: Connected and logged in", camera_config.name);

            let talk_ability = camera.talk_ability()?;
            if talk_ability.duplex_list.is_empty()
                || talk_ability.audio_stream_mode_list.is_empty()
                || talk_ability.audio_config_list.is_empty()
            {
                return Err(Error::TalkUnsupported);
            }

            // Just copy that data from the first talk ability in the config have never seen more
            // than one ability
            let config = 0;

            let talk_config = TalkConfig {
                channel_id: camera_config.channel_id,
                duplex: talk_ability.duplex_list[config].duplex.clone(),
                audio_stream_mode: talk_ability.audio_stream_mode_list[config]
                    .audio_stream_mode
                    .clone(),
                audio_config: talk_ability.audio_config_list[config].audio_config.clone(),
                ..Default::default()
            };

            let block_size = (talk_config.audio_config.length_per_encoder / 2) + 4;
            let sample_rate = talk_config.audio_config.sample_rate;
            if block_size == 0 || sample_rate == 0 {
                return Err(Error::TalkUnsupported);
            }

            let rx = match (&opt.file_path, &opt.microphone) {
                (Some(path), false) => gst::from_input(
                    &format!("filesrc location={}", path.to_str().expect("File path not UTF8 complient")),
                    opt.volume,
                    block_size,
                    sample_rate,
                )?,
                (None, true) => gst::from_input(&opt.input_src, opt.volume, block_size, sample_rate)?,
                _ => unreachable!(),
            };

            camera.talk_stream(rx, talk_config)?;
        }
    }

    if !cam_found {
        error!(
            "No camera with the name {} was found in the config",
            opt.camera
        );
    }

    Ok(())
}

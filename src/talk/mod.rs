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
/// neolink talk --config=config.toml --adpcm-file=data.adpcm --sample-rate=16000 --block-size=512 CameraName
/// ```
///
use anyhow::{anyhow, Context, Result};
use neolink_core::bc::xml::TalkConfig;

mod cmdline;
mod gst;

use super::config::Config;
use crate::utils::{connect_and_login, find_camera_by_name};
pub(crate) use cmdline::Opt;

/// Entry point for the talk subcommand
///
/// Opt is the command line options
pub(crate) async fn main(opt: Opt, config: Config) -> Result<()> {
    let camera_config = find_camera_by_name(&config, &opt.camera)?;
    let camera = connect_and_login(camera_config).await?;

    let talk_ability = camera
        .talk_ability()
        .await
        .with_context(|| format!("Camera {} does not support talk", camera_config.name))?;
    if talk_ability.duplex_list.is_empty()
        || talk_ability.audio_stream_mode_list.is_empty()
        || talk_ability.audio_config_list.is_empty()
    {
        return Err(anyhow!(
            "Camera {} does not support talk",
            camera_config.name
        ));
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
        return Err(anyhow!(
            "The camera {} does not support talk with adpcm",
            camera_config.name
        ));
    }

    let rx = match (&opt.file_path, &opt.microphone) {
        (Some(path), false) => gst::from_input(
            &format!(
                "filesrc location={}",
                path.to_str().expect("File path not UTF8 complient")
            ),
            opt.volume,
            block_size,
            sample_rate,
        )
        .with_context(|| format!("Failed to setup gst with the file: {:?}", path))?,
        (None, true) => gst::from_input(&opt.input_src, opt.volume, block_size, sample_rate)
            .context("Failed to setup gst using the microphone")?,
        _ => unreachable!(),
    };

    camera
        .talk_stream(rx, talk_config)
        .await
        .context("Talk stream ended early")?;

    Ok(())
}

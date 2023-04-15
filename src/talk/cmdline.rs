use clap::Parser;
use std::path::PathBuf;
use std::str::FromStr;

/// The talk command will send audio for the camera to say
///
/// This data should be encoded in a way that gstreamer can understand.
/// This should be ok with most common formats.
///
/// `gst-launch` can be used to prepare this data
#[derive(Parser, Debug)]
pub struct Opt {
    /// The name of the camera to talk through. Must be a name in the config
    pub camera: String,
    /// The path to the audio file.
    #[arg(short, long, value_parser = PathBuf::from_str, conflicts_with = "microphone")]
    pub file_path: Option<PathBuf>,
    /// Use the microphone as the source. Defaults to autoaudiosrc - Which microphone depends
    /// on [gstreamer](https://gstreamer.freedesktop.org/documentation/autodetect/autoaudiosrc.html?gi-language=c#autoaudiosrc-page)
    #[arg(short, long, conflicts_with = "file_path")]
    pub microphone: bool,
    /// Use a specific microphone like "alsasrc device=hw:1"
    #[arg(
        short,
        long,
        default_value = "autoaudiosrc",
        conflicts_with = "file_path"
    )]
    pub input_src: String,
    /// Use to change the volume of the input
    #[arg(short, long, default_value = "1.0")]
    pub volume: f32,
}

use std::path::PathBuf;
use structopt::StructOpt;

/// The talk command will send audio for the camera to say
///
/// This data should be encoded in a way that gstreamer can understand.
/// This should be ok with most common formats.
///
/// `gst-launch` can be used to prepare this data
#[derive(StructOpt, Debug)]
pub struct Opt {
    /// The name of the camera to change the lights of. Must be a name in the config
    pub camera: String,
    /// The path to the audio file.
    #[structopt(short, long, parse(from_os_str), conflicts_with = "microphone")]
    pub file_path: Option<PathBuf>,
    /// Use the microphone as the source. Defaults to autoaudiosrc - Which microphone depends
    /// on [gstraemer](https://gstreamer.freedesktop.org/documentation/autodetect/autoaudiosrc.html?gi-language=c#autoaudiosrc-page)
    #[structopt(short, long, conflicts_with = "file_path")]
    pub microphone: bool,
    /// Use a specific microphone like "alsasrc device=hw:1"
    #[structopt(
        short,
        long,
        default_value = "autoaudiosrc",
        conflicts_with = "file_path"
    )]
    pub input_src: String,
    /// Use to change the volume of the input
    #[structopt(short, long, default_value = "1.0")]
    pub volume: f32,
}

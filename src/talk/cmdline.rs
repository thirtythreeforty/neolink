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
    /// The path to the config file
    #[structopt(short, long, parse(from_os_str))]
    pub config: PathBuf,
    /// The name of the camera to change the lights of. Must be a name in the config
    pub camera: String,
    /// The path to the audio file.
    #[structopt(short, long, parse(from_os_str))]
    pub media_path: PathBuf,
}

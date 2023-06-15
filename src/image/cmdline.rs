use clap::Parser;
use std::path::PathBuf;
use std::str::FromStr;

/// The image command will dump a still image from the camera
#[derive(Parser, Debug)]
pub struct Opt {
    /// The name of the camera to get the image from. Must be a name in the config
    pub camera: String,
    /// The path of the output.
    #[structopt(short, long, value_parser = PathBuf::from_str)]
    pub file_path: PathBuf,
    /// If set then the image will pull from the live stream, if not it will be pulled from the cameras snap feature
    ///
    /// Using the snap feature, is preffered unless your camera does not support it
    #[structopt(short, long)]
    pub use_stream: bool,
}

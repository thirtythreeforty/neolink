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
}

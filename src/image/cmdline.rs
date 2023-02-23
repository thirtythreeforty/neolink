use std::path::PathBuf;
use structopt::StructOpt;

/// The image command will dump a still image from the camera
#[derive(StructOpt, Debug)]
pub struct Opt {
    /// The name of the camera to get the image from. Must be a name in the config
    pub camera: String,
    /// The path of the output.
    #[structopt(short, long, parse(from_os_str))]
    pub file_path: PathBuf,
}

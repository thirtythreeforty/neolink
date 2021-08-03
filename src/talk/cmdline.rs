use std::path::PathBuf;
use structopt::StructOpt;

/// The talk command will send adpcm data to the camera to say
///
/// This data should be encoded in adpcm with dvi4 layout
///
/// `gst-launch` can be used to prepare this data
#[derive(StructOpt, Debug)]
pub struct Opt {
    /// The path to the config file
    #[structopt(short, long, parse(from_os_str))]
    pub config: PathBuf,
    /// The name of the camera to change the lights of. Must be a name in the config
    pub camera: String,
    /// The path to the adpcm data to send in DVI-4 layout
    #[structopt(short, long, parse(from_os_str), name = "media-path")]
    pub adpcm_file: PathBuf,
    /// The block size used to encode the adpcm data (recommended 512)
    #[structopt(short, long)]
    pub block_size: u16,
    /// The sample rate used to encode the adpcm data (recommended 16000)
    #[structopt(short, long)]
    pub sample_rate: u16,
}

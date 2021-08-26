use std::path::PathBuf;
use structopt::StructOpt;

/// The rtsp command will serve all cameras in the config over the rtsp protocol
#[derive(StructOpt, Debug)]
pub struct Opt {
    /// The path to the config file
    #[structopt(short, long, parse(from_os_str))]
    pub config: PathBuf,
}

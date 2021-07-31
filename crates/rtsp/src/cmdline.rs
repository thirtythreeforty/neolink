use std::path::PathBuf;
use structopt::StructOpt;

/// The command line for the rtsp subcommand
#[derive(StructOpt, Debug)]
pub struct Opt {
    /// The path to the config file
    #[structopt(short, long, parse(from_os_str))]
    pub config: PathBuf,
}

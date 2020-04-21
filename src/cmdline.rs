use std::path::PathBuf;
use structopt::StructOpt;

/// A standards-compliant bridge to Reolink IP cameras
#[derive(StructOpt, Debug)]
#[structopt(name = "neolink")]
pub struct Opt {
    /// main configuration file
    #[structopt(short, long, parse(from_os_str))]
    pub config: PathBuf,
}

use structopt::StructOpt;

/// A standards-compliant bridge to Reolink IP cameras
#[derive(StructOpt, Debug)]
#[structopt(name = "neolink")]
pub struct Opt {
    #[structopt(subcommand)]
    pub cmd: Command,
}

#[derive(StructOpt, Debug)]
pub enum Command {
    Rtsp(neolink_rtsp::cmdline::Opt),
}

use env_logger::Env;
use log::*;
use structopt::StructOpt;

mod cmdline;
mod errors;

use cmdline::{Command, Opt};
use errors::Error;

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!(
        "Neolink {} {}",
        env!("NEOLINK_VERSION"),
        env!("NEOLINK_PROFILE")
    );

    let opt = Opt::from_args();

    match opt.cmd {
        Command::Rtsp(opts) => {
            neolink_rtsp::main(opts)?;
        }
    }

    Ok(())
}

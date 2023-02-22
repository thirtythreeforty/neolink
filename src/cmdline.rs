use std::path::PathBuf;
use structopt::{clap::AppSettings, StructOpt};

/// A standards-compliant bridge to Reolink IP cameras
///
/// Neolink is free software released under the GNU AGPL v3.
/// You can find its source code at https://github.com/thirtythreeforty/neolink
#[derive(StructOpt, Debug)]
#[structopt(
    name = "neolink",
    setting(AppSettings::ArgRequiredElseHelp),
    setting(AppSettings::UnifiedHelpMessage)
)]
pub struct Opt {
    #[structopt(short, long, global(true), parse(from_os_str))]
    pub config: Option<PathBuf>,
    #[structopt(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(StructOpt, Debug)]
pub enum Command {
    Rtsp(super::rtsp::Opt),
    StatusLight(super::statusled::Opt),
    Reboot(super::reboot::Opt),
    Pir(super::pir::Opt),
    Talk(super::talk::Opt),
}

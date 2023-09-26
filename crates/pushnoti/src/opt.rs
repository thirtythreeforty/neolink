use clap::{crate_authors, crate_version, Parser};
use std::path::PathBuf;
use std::str::FromStr;

/// A standards-compliant bridge to Reolink IP cameras
///
/// Neolink is free software released under the GNU AGPL v3.
/// You can find its source code at https://github.com/thirtythreeforty/neolink
#[derive(Parser, Debug)]
#[command(name = "pushnoti", arg_required_else_help = true, version = crate_version!(), author = crate_authors!("\n"))]
pub struct Opt {
    #[arg(short, long, value_parser = PathBuf::from_str)]
    pub config: Option<PathBuf>,
    /// The name of the camera. Must be a name in the config
    pub camera: String,
}

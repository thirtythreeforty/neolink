use super::errors::ParseOnOffError;
use std::path::PathBuf;
use structopt::StructOpt;

fn onoff_parse(src: &str) -> Result<bool, ParseOnOffError> {
    match src {
        "true" | "on" | "yes" => Ok(true),
        "false" | "off" | "no" => Ok(false),
        _ => Err(ParseOnOffError),
    }
}

/// The status-light command will control the blue status light on the camera
#[derive(StructOpt, Debug)]
pub struct Opt {
    /// The path to the config file
    #[structopt(short, long, parse(from_os_str))]
    pub config: PathBuf,
    /// The name of the camera to change the lights of. Must be a name in the config
    pub camera: String,
    /// Whether to turn the light on or off
    #[structopt(parse(try_from_str = onoff_parse), name = "on|off")]
    pub on: bool,
}

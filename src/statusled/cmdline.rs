use anyhow::{anyhow, Result};
use clap::Parser;

fn onoff_parse(src: &str) -> Result<bool> {
    match src {
        "true" | "on" | "yes" => Ok(true),
        "false" | "off" | "no" => Ok(false),
        _ => Err(anyhow!(
            "Could not understand {}, check your input, should be true/false, on/off or yes/no",
            src
        )),
    }
}

/// The status-light command will control the blue status light on the camera
#[derive(Parser, Debug)]
pub struct Opt {
    /// The name of the camera to change the lights of. Must be a name in the config
    pub camera: String,
    /// Whether to turn the light on or off
    #[arg(value_parser = onoff_parse, name = "on|off")]
    pub on: bool,
}

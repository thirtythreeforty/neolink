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

/// The pir command will control the PIR status of the camera
#[derive(Parser, Debug)]
pub struct Opt {
    /// The name of the camera. Must be a name in the config
    pub camera: String,
    /// Whether to turn the PIR ON or OFF
    #[arg(value_parser = onoff_parse, name = "on|off")]
    pub on: Option<bool>,
}

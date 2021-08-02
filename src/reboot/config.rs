use serde::Deserialize;
use std::clone::Clone;
use validator::Validate;
use validator_derive::Validate;

#[derive(Debug, Deserialize, Validate, Clone)]
pub(crate) struct Config {
    #[validate]
    pub(crate) cameras: Vec<CameraConfig>,
}

#[derive(Debug, Deserialize, Validate, Clone)]
pub(crate) struct CameraConfig {
    pub(crate) name: String,

    #[serde(rename = "address")]
    pub(crate) camera_addr: String,

    pub(crate) username: String,
    pub(crate) password: Option<String>,

    #[validate(range(min = 0, max = 31, message = "Invalid channel", code = "channel_id"))]
    #[serde(default = "default_channel_id")]
    pub(crate) channel_id: u8,
}

fn default_channel_id() -> u8 {
    0
}

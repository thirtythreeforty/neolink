use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use std::clone::Clone;
use std::time::Duration;
use validator::{Validate, ValidationError};
use validator_derive::Validate;

lazy_static! {
    static ref RE_STREAM_SRC: Regex =
        Regex::new(r"^(mainStream|subStream|externStream|both|all)$").unwrap();
    static ref RE_TLS_CLIENT_AUTH: Regex = Regex::new(r"^(none|request|require)$").unwrap();
}

#[derive(Debug, Deserialize, Validate, Clone)]
pub(crate) struct Config {
    #[validate]
    pub(crate) cameras: Vec<CameraConfig>,

    #[serde(rename = "bind", default = "default_bind_addr")]
    pub(crate) bind_addr: String,

    #[validate(range(min = 0, max = 65535, message = "Invalid port", code = "bind_port"))]
    #[serde(default = "default_bind_port")]
    pub(crate) bind_port: u16,

    #[serde(default = "default_certificate")]
    pub(crate) certificate: Option<String>,

    #[validate(regex(
        path = "RE_TLS_CLIENT_AUTH",
        message = "Incorrect tls auth",
        code = "tls_client_auth"
    ))]
    #[serde(default = "default_tls_client_auth")]
    pub(crate) tls_client_auth: String,

    #[validate]
    #[serde(default)]
    pub(crate) users: Vec<UserConfig>,
}

#[derive(Debug, Deserialize, Validate, Clone)]
#[validate(schema(function = "validate_camera_config"))]
pub(crate) struct CameraConfig {
    pub(crate) name: String,

    #[serde(rename = "address")]
    pub(crate) camera_addr: Option<String>,

    #[serde(rename = "uid")]
    pub(crate) camera_uid: Option<String>,

    pub(crate) username: String,
    pub(crate) password: Option<String>,

    // no longer used, but still here so we can warn users:
    pub(crate) timeout: Option<Duration>,

    // no longer used, but still here so we can warn users:
    pub(crate) format: Option<String>,

    #[validate(regex(
        path = "RE_STREAM_SRC",
        message = "Incorrect stream source",
        code = "stream"
    ))]
    #[serde(default = "default_stream")]
    pub(crate) stream: String,

    pub(crate) permitted_users: Option<Vec<String>>,

    #[validate(range(min = 0, max = 31, message = "Invalid channel", code = "channel_id"))]
    #[serde(default = "default_channel_id")]
    pub(crate) channel_id: u8,

    #[validate]
    #[serde(default = "default_mqtt")]
    pub(crate) mqtt: Option<MqttConfig>,
}

#[derive(Debug, Deserialize, Validate, Clone)]
pub(crate) struct UserConfig {
    #[validate(custom = "validate_username")]
    #[serde(alias = "username")]
    pub(crate) name: String,

    #[serde(alias = "password")]
    pub(crate) pass: String,
}

#[derive(Debug, Deserialize, Clone, Validate)]
#[validate(schema(function = "validate_mqtt_config", skip_on_field_errors = true))]
pub(crate) struct MqttConfig {
    #[serde(alias = "server")]
    pub(crate) broker_addr: String,

    pub(crate) port: u16,

    #[serde(default)]
    pub(crate) credentials: Option<(String, String)>,

    #[serde(default)]
    pub(crate) ca: Option<std::path::PathBuf>,

    #[serde(default)]
    pub(crate) client_auth: Option<(std::path::PathBuf, std::path::PathBuf)>,
}

fn validate_mqtt_config(config: &MqttConfig) -> Result<(), ValidationError> {
    if config.ca.is_some() && config.client_auth.is_some() {
        Err(ValidationError::new(
            "Cannot have both ca and client_auth set",
        ))
    } else {
        Ok(())
    }
}

fn default_mqtt() -> Option<MqttConfig> {
    None
}

fn default_bind_addr() -> String {
    "0.0.0.0".to_string()
}

fn default_bind_port() -> u16 {
    8554
}

fn default_stream() -> String {
    "both".to_string()
}

fn default_certificate() -> Option<String> {
    None
}

fn default_tls_client_auth() -> String {
    "none".to_string()
}

fn default_channel_id() -> u8 {
    0
}

pub(crate) static RESERVED_NAMES: &[&str] = &["anyone", "anonymous"];
fn validate_username(name: &str) -> Result<(), ValidationError> {
    if name.trim().is_empty() {
        return Err(ValidationError::new("username cannot be empty"));
    }
    if RESERVED_NAMES.contains(&name) {
        return Err(ValidationError::new("This is a reserved username"));
    }
    Ok(())
}

fn validate_camera_config(camera_config: &CameraConfig) -> Result<(), ValidationError> {
    match (&camera_config.camera_addr, &camera_config.camera_uid) {
        (None, None) => Err(ValidationError::new(
            "Either camera address or uid must be given",
        )),
        (Some(_), Some(_)) => Err(ValidationError::new(
            "Must provide either camera address or uid not both",
        )),
        _ => Ok(()),
    }
}

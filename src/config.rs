use crate::mqtt::Discoveries;
use neolink_core::bc_protocol::{DiscoveryMethods, PrintFormat, StreamKind};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::clone::Clone;
use std::collections::HashSet;
use validator::ValidationError;
use validator_derive::Validate;

static RE_TLS_CLIENT_AUTH: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(none|request|require)$").unwrap());
static RE_PAUSE_MODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(black|still|test|none)$").unwrap());
static RE_MAXENC_SRC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^([nN]one|[Aa][Ee][Ss]|[Bb][Cc][Ee][Nn][Cc][Rr][Yy][Pp][Tt])$").unwrap()
});

#[derive(Debug, Deserialize, Serialize, Validate, Clone, PartialEq)]
pub(crate) struct Config {
    #[validate]
    pub(crate) cameras: Vec<CameraConfig>,

    #[serde(rename = "bind", default = "default_bind_addr")]
    pub(crate) bind_addr: String,

    #[validate(range(min = 0, max = 65535, message = "Invalid port", code = "bind_port"))]
    #[serde(default = "default_bind_port")]
    pub(crate) bind_port: u16,

    #[serde(default = "default_tokio_console")]
    pub(crate) tokio_console: bool,

    #[serde(default = "default_certificate")]
    pub(crate) certificate: Option<String>,

    #[serde(default = "Default::default")]
    pub(crate) mqtt: Option<MqttServerConfig>,

    #[validate(regex(
        path = *RE_TLS_CLIENT_AUTH,
        message = "Incorrect tls auth",
        code = "tls_client_auth"
    ))]
    #[serde(default = "default_tls_client_auth")]
    pub(crate) tls_client_auth: String,

    #[validate]
    #[serde(default)]
    pub(crate) users: Vec<UserConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Validate, PartialEq, Eq)]
#[validate(schema(function = "validate_mqtt_server", skip_on_field_errors = true))]
pub(crate) struct MqttServerConfig {
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

#[derive(Debug, Deserialize, Serialize, Clone, Copy, Eq, PartialEq)]
pub(crate) enum StreamConfig {
    #[serde(alias = "none")]
    None,
    #[serde(alias = "all")]
    All,
    #[serde(alias = "both")]
    Both,
    #[serde(
        alias = "main",
        alias = "mainStream",
        alias = "mainstream",
        alias = "MainStream"
    )]
    Main,
    #[serde(
        alias = "sub",
        alias = "subStream",
        alias = "substream",
        alias = "SubStream"
    )]
    Sub,
    #[serde(
        alias = "extern",
        alias = "externStream",
        alias = "externstream",
        alias = "ExternStream"
    )]
    Extern,
}

impl StreamConfig {
    pub(crate) fn as_stream_kinds(&self) -> Vec<StreamKind> {
        match self {
            StreamConfig::All => {
                vec![StreamKind::Main, StreamKind::Extern, StreamKind::Sub]
            }
            StreamConfig::Both => {
                vec![StreamKind::Main, StreamKind::Sub]
            }
            StreamConfig::Main => {
                vec![StreamKind::Main]
            }
            StreamConfig::Sub => {
                vec![StreamKind::Sub]
            }
            StreamConfig::Extern => {
                vec![StreamKind::Extern]
            }
            StreamConfig::None => {
                vec![]
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Validate, Clone, PartialEq)]
#[validate(schema(function = "validate_camera_config"))]
pub(crate) struct CameraConfig {
    pub(crate) name: String,

    #[serde(rename = "address")]
    pub(crate) camera_addr: Option<String>,

    #[serde(rename = "uid")]
    pub(crate) camera_uid: Option<String>,

    pub(crate) username: String,
    pub(crate) password: Option<String>,

    #[serde(default = "default_stream")]
    pub(crate) stream: StreamConfig,

    pub(crate) permitted_users: Option<Vec<String>>,

    #[validate(range(min = 0, max = 31, message = "Invalid channel", code = "channel_id"))]
    #[serde(default = "default_channel_id", alias = "channel")]
    pub(crate) channel_id: u8,

    #[validate]
    #[serde(default = "default_mqtt")]
    pub(crate) mqtt: MqttConfig,

    #[validate]
    #[serde(default = "default_pause")]
    pub(crate) pause: PauseConfig,

    #[serde(default = "default_discovery")]
    pub(crate) discovery: DiscoveryMethods,

    #[serde(default = "default_maxenc")]
    #[validate(regex(
        path = *RE_MAXENC_SRC,
        message = "Invalid maximum encryption method",
        code = "max_encryption"
    ))]
    pub(crate) max_encryption: String,

    #[serde(default = "default_strict")]
    /// If strict then the media stream will error in the event that the media packets are not as expected
    pub(crate) strict: bool,

    #[serde(default = "default_print", alias = "print")]
    pub(crate) print_format: PrintFormat,

    #[serde(default = "default_update_time", alias = "time")]
    pub(crate) update_time: bool,

    #[validate(range(
        min = 0,
        max = 500,
        message = "Invalid buffer size",
        code = "buffer_size"
    ))]
    #[serde(default = "default_buffer_size", alias = "size", alias = "buffer")]
    pub(crate) buffer_size: usize,

    #[serde(default = "default_true", alias = "enable")]
    pub(crate) enabled: bool,

    #[serde(default = "default_false", alias = "verbose")]
    pub(crate) debug: bool,

    #[serde(default = "default_true", alias = "splash")]
    pub(crate) use_splash: bool,

    #[serde(default = "default_splash", alias = "pattern")]
    pub(crate) splash_pattern: SplashPattern,

    #[serde(
        default = "default_max_discovery_retries",
        alias = "retries",
        alias = "max_retries"
    )]
    pub(crate) max_discovery_retries: usize,

    #[serde(default = "default_true", alias = "push", alias = "push_noti")]
    pub(crate) push_notifications: bool,

    #[serde(default = "default_false", alias = "idle", alias = "idle_disc")]
    pub(crate) idle_disconnect: bool,
}

#[derive(Debug, Deserialize, Serialize, Validate, Clone, PartialEq, Eq, Hash)]
pub(crate) struct UserConfig {
    #[validate(custom(function = "validate_username"))]
    #[serde(alias = "username")]
    pub(crate) name: String,

    #[serde(alias = "password")]
    pub(crate) pass: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Validate, PartialEq, Eq)]
pub(crate) struct MqttConfig {
    #[serde(default = "default_true")]
    pub(crate) enable_motion: bool,
    #[serde(default = "default_true")]
    pub(crate) enable_light: bool,
    #[serde(default = "default_true")]
    pub(crate) enable_battery: bool,
    /// Update time in ms
    #[serde(default = "default_2000")]
    #[validate(range(
        min = 500,
        message = "Update ms should be > 500",
        code = "battery_update"
    ))]
    pub(crate) battery_update: u64,
    #[serde(default = "default_true")]
    pub(crate) enable_preview: bool,
    /// Update time in ms
    #[validate(range(
        min = 500,
        message = "Update ms should be > 500",
        code = "preview_update"
    ))]
    #[serde(default = "default_2000")]
    pub(crate) preview_update: u64,

    /// Enable the flood light tasks status
    /// Will not do anything if no floodlight
    /// is detected
    #[serde(default = "default_true")]
    pub(crate) enable_floodlight: bool,
    /// Update time in ms
    #[validate(range(
        min = 500,
        message = "Update ms should be > 500",
        code = "floodlight_update"
    ))]
    #[serde(default = "default_2000")]
    pub(crate) floodlight_update: u64,

    #[serde(default)]
    pub(crate) discovery: Option<MqttDiscoveryConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Validate, PartialEq, Eq)]
pub(crate) struct MqttDiscoveryConfig {
    pub(crate) topic: String,

    pub(crate) features: HashSet<Discoveries>,
}

fn validate_mqtt_server(config: &MqttServerConfig) -> Result<(), ValidationError> {
    if config.ca.is_some() && config.client_auth.is_some() {
        Err(ValidationError::new(
            "Cannot have both ca and client_auth set",
        ))
    } else {
        Ok(())
    }
}

const fn default_true() -> bool {
    true
}

const fn default_false() -> bool {
    false
}

fn default_mqtt() -> MqttConfig {
    MqttConfig {
        enable_motion: true,
        enable_light: true,
        enable_battery: true,
        battery_update: 2000,
        enable_preview: true,
        preview_update: 2000,
        enable_floodlight: true,
        floodlight_update: 2000,
        discovery: Default::default(),
    }
}

fn default_print() -> PrintFormat {
    PrintFormat::None
}

fn default_discovery() -> DiscoveryMethods {
    DiscoveryMethods::Relay
}

fn default_maxenc() -> String {
    "Aes".to_string()
}

#[derive(Debug, Deserialize, Serialize, Validate, Clone, PartialEq)]
pub(crate) struct PauseConfig {
    #[serde(default = "default_on_motion")]
    pub(crate) on_motion: bool,

    #[serde(default = "default_on_disconnect", alias = "on_client")]
    pub(crate) on_disconnect: bool,

    #[serde(default = "default_motion_timeout", alias = "timeout")]
    pub(crate) motion_timeout: f64,

    #[serde(default = "default_pause_mode")]
    #[validate(regex(
        path = *RE_PAUSE_MODE,
        message = "Incorrect pause mode",
        code = "mode"
    ))]
    pub(crate) mode: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, Eq, PartialEq)]
pub(crate) enum SplashPattern {
    #[serde(alias = "smpte")]
    Smpte,
    #[serde(alias = "snow")]
    Snow,
    #[serde(alias = "black")]
    Black,
    #[serde(alias = "white")]
    White,
    #[serde(alias = "red")]
    Red,
    #[serde(alias = "green")]
    Green,
    #[serde(alias = "blue")]
    Blue,
    #[serde(alias = "checkers-1")]
    Checkers1,
    #[serde(alias = "checkers-2")]
    Checkers2,
    #[serde(alias = "checkers-4")]
    Checkers4,
    #[serde(alias = "checkers-8")]
    Checkers8,
    #[serde(alias = "circular")]
    Circular,
    #[serde(alias = "blink")]
    Blink,
    #[serde(alias = "smpte75")]
    Smpte75,
    #[serde(alias = "zone-plate")]
    ZonePlate,
    #[serde(alias = "gamut")]
    Gamut,
    #[serde(alias = "chroma-zone-plate")]
    ChromaZonePlate,
    #[serde(alias = "solid-color")]
    SolidColor,
    #[serde(alias = "ball")]
    Ball,
    #[serde(alias = "smpte100")]
    Smpte100,
    #[serde(alias = "bar")]
    Bar,
    #[serde(alias = "pinwheel")]
    Pinwheel,
    #[serde(alias = "spokes")]
    Spokes,
    #[serde(alias = "gradient")]
    Gradient,
    #[serde(alias = "colors")]
    Colors,
    #[serde(alias = "smpte-rp-219")]
    SmpteRp219,
}

impl std::fmt::Display for SplashPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s = match self {
            SplashPattern::Smpte => "smpte",
            SplashPattern::Snow => "snow",
            SplashPattern::Black => "black",
            SplashPattern::White => "white",
            SplashPattern::Red => "red",
            SplashPattern::Green => "green",
            SplashPattern::Blue => "blue",
            SplashPattern::Checkers1 => "checkers-1",
            SplashPattern::Checkers2 => "checkers-2",
            SplashPattern::Checkers4 => "checkers-4",
            SplashPattern::Checkers8 => "checkers-8",
            SplashPattern::Circular => "circular",
            SplashPattern::Blink => "blink",
            SplashPattern::Smpte75 => "smpte75",
            SplashPattern::ZonePlate => "zone-plate",
            SplashPattern::Gamut => "gamut",
            SplashPattern::ChromaZonePlate => "chroma-zone-plate",
            SplashPattern::SolidColor => "solid-color",
            SplashPattern::Ball => "ball",
            SplashPattern::Smpte100 => "smpte100",
            SplashPattern::Bar => "bar",
            SplashPattern::Pinwheel => "pinwheel",
            SplashPattern::Spokes => "spokes",
            SplashPattern::Gradient => "gradient",
            SplashPattern::Colors => "colors",
            SplashPattern::SmpteRp219 => "smpte-rp-219",
        }
        .to_string();
        write!(f, "{}", s)
    }
}

fn default_bind_addr() -> String {
    "0.0.0.0".to_string()
}

fn default_bind_port() -> u16 {
    8554
}

fn default_stream() -> StreamConfig {
    StreamConfig::All
}

fn default_certificate() -> Option<String> {
    None
}

fn default_tls_client_auth() -> String {
    "none".to_string()
}

fn default_tokio_console() -> bool {
    false
}

fn default_channel_id() -> u8 {
    0
}

fn default_update_time() -> bool {
    false
}

fn default_motion_timeout() -> f64 {
    1.
}

fn default_on_disconnect() -> bool {
    false
}

fn default_on_motion() -> bool {
    false
}

fn default_pause_mode() -> String {
    "none".to_string()
}

fn default_strict() -> bool {
    false
}

fn default_pause() -> PauseConfig {
    PauseConfig {
        on_motion: default_on_motion(),
        on_disconnect: default_on_disconnect(),
        motion_timeout: default_motion_timeout(),
        mode: default_pause_mode(),
    }
}

fn default_buffer_size() -> usize {
    25
}

fn default_max_discovery_retries() -> usize {
    10
}

fn default_2000() -> u64 {
    2000
}

fn default_splash() -> SplashPattern {
    SplashPattern::Snow
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
        _ => Ok(()),
    }
}

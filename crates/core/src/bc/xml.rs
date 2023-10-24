// YaSerde currently macro-expands names like __type__value from type_
#![allow(non_snake_case)]

use std::io::{Read, Write};
// YaSerde is currently naming the traits and the derive macros identically
use yaserde::ser::Config;
use yaserde_derive::{YaDeserialize, YaSerialize};

#[cfg(test)]
use indoc::indoc;

/// There are two types of payloads xml and binary
#[derive(PartialEq, Debug, YaDeserialize)]
#[yaserde(flatten)]
#[allow(clippy::large_enum_variant)]
pub enum BcPayloads {
    /// XML payloads are the more common ones and include payloads for camera controls
    #[yaserde(rename = "body")]
    BcXml(BcXml),
    /// Binary payloads are received from the camera for streams and sent to the camera
    /// for talk-back and firmware updates
    #[yaserde(flatten)]
    Binary(Vec<u8>),
}

// Required for YaDeserialize
impl Default for BcPayloads {
    fn default() -> Self {
        BcPayloads::Binary(Default::default())
    }
}

/// The top level BC Xml
#[derive(PartialEq, Default, Debug, YaDeserialize, YaSerialize)]
#[yaserde(rename = "body")]
pub struct BcXml {
    /// Encryption xml is received during login and contain the NONCE
    #[yaserde(rename = "Encryption")]
    pub encryption: Option<Encryption>,
    /// LoginUser xml is used during modern login
    #[yaserde(rename = "LoginUser")]
    pub login_user: Option<LoginUser>,
    /// LoginNet xml is used during modern login
    #[yaserde(rename = "LoginNet")]
    pub login_net: Option<LoginNet>,
    /// The final part of a login sequence will return DeviceInfo xml
    #[yaserde(rename = "DeviceInfo")]
    pub device_info: Option<DeviceInfo>,
    /// The VersionInfo xml is recieved in reply to a version request
    #[yaserde(rename = "VersionInfo")]
    pub version_info: Option<VersionInfo>,
    /// Preview xml is used as part of the stream request to set the stream quality and channel
    #[yaserde(rename = "Preview")]
    pub preview: Option<Preview>,
    #[yaserde(rename = "SystemGeneral")]
    /// SystemGeneral xml is sent or recieved as part of the clock get/setting
    pub system_general: Option<SystemGeneral>,
    /// Received as part of the Genral system info request
    #[yaserde(rename = "Norm")]
    pub norm: Option<Norm>,
    /// Received as part of the LEDState info request
    #[yaserde(rename = "LedState")]
    pub led_state: Option<LedState>,
    /// Sent as part of the TalkConfig to prepare the camera for audio talk-back
    #[yaserde(rename = "TalkConfig")]
    pub talk_config: Option<TalkConfig>,
    /// rfAlarmCfg xml is sent or recieved as part of the PIR get/setting
    #[yaserde(rename = "rfAlarmCfg")]
    pub rf_alarm_cfg: Option<RfAlarmCfg>,
    /// Revieced as part of the TalkAbility request
    #[yaserde(rename = "TalkAbility")]
    pub talk_ability: Option<TalkAbility>,
    /// Received when motion is detected
    #[yaserde(rename = "AlarmEventList")]
    pub alarm_event_list: Option<AlarmEventList>,
    /// Sent to move the camera
    #[yaserde(rename = "PtzControl")]
    pub ptz_control: Option<PtzControl>,
    /// Sent to manually control the floodlight
    #[yaserde(rename = "FloodlightManual")]
    pub floodlight_manual: Option<FloodlightManual>,
    /// Received when the floodlight status is updated
    #[yaserde(rename = "FloodlightStatusList")]
    pub floodlight_status_list: Option<FloodlightStatusList>,
    /// Sent or received for the PTZ preset functionality
    #[yaserde(rename = "PtzPreset")]
    pub ptz_preset: Option<PtzPreset>,
    /// Recieved on login/low battery events
    #[yaserde(rename = "BatteryList")]
    pub battery_list: Option<BatteryList>,
    /// Recieved on request for battery info
    #[yaserde(rename = "BatteryInfo")]
    pub battery_info: Option<BatteryInfo>,
    /// Recieved on request for a users persmissions/capabilitoes
    #[yaserde(rename = "AbilityInfo")]
    pub ability_info: Option<AbilityInfo>,
    /// Recieved on request for a users persmissions/capabilitoes
    #[yaserde(rename = "PushInfo")]
    pub push_info: Option<PushInfo>,
    /// Recieved on request for a link type
    #[yaserde(rename = "LinkType")]
    pub link_type: Option<LinkType>,
    /// Recieved AND send for the snap message
    #[yaserde(rename = "Snap")]
    pub snap: Option<Snap>,
    /// The list of streams and their configuration
    #[yaserde(rename = "StreamInfoList")]
    pub stream_info_list: Option<StreamInfoList>,
    /// Thre list of streams and their configuration
    #[yaserde(rename = "Uid")]
    pub uid: Option<Uid>,
    /// The floodlight settings for automatically turning on/off on schedule/motion
    #[yaserde(rename = "FloodlightTask")]
    pub floodlight_task: Option<FloodlightTask>,
    /// For zooming the camera
    #[yaserde(rename = "StartZoomFocus")]
    pub start_zoom_focus: Option<StartZoomFocus>,
    /// Get the support xml
    #[yaserde(rename = "Support")]
    pub support: Option<Support>,
}

impl BcXml {
    pub(crate) fn try_parse(s: impl Read) -> Result<Self, String> {
        yaserde::de::from_reader(s)
    }
    pub(crate) fn serialize<W: Write>(&self, w: W) -> Result<W, String> {
        yaserde::ser::serialize_with_writer(self, w, &Config::default())
    }
}

impl Extension {
    pub(crate) fn try_parse(s: impl Read) -> Result<Self, String> {
        yaserde::de::from_reader(s)
    }
    pub(crate) fn serialize<W: Write>(&self, w: W) -> Result<W, String> {
        yaserde::ser::serialize_with_writer(self, w, &Config::default())
    }
}

/// Encryption xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Encryption {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    #[yaserde(rename = "type")]
    /// The hashing algorithm used. Only observed the value of "md5"
    pub type_: String,
    /// The nonce used to negotiate the login and to generate the AES key
    pub nonce: String,
}

/// LoginUser xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct LoginUser {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Username to login as
    #[yaserde(rename = "userName")]
    pub user_name: String,
    /// Password for login in plain text
    pub password: String,
    /// Unknown always `1`
    #[yaserde(rename = "userVer")]
    pub user_ver: u32,
}

/// LoginNet xml
#[derive(PartialEq, Eq, Debug, YaDeserialize, YaSerialize)]
pub struct LoginNet {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Type of connection usually LAN (even on wifi)
    #[yaserde(rename = "type")]
    pub type_: String,
    /// The port for the udp will be `0` for tcp
    #[yaserde(rename = "udpPort")]
    pub udp_port: u16,
}

impl Default for LoginNet {
    fn default() -> Self {
        LoginNet {
            version: xml_ver(),
            type_: "LAN".to_string(),
            udp_port: 0,
        }
    }
}

/// DeviceInfo xml
///
/// There is more to this xml but we don't deserialize it all
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct DeviceInfo {
    /// The resolution xml block
    pub resolution: Resolution,
}

/// VersionInfo xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct VersionInfo {
    /// Name assigned to the camera
    pub name: String,
    /// Model Name
    #[yaserde(rename = "type")]
    pub model: Option<String>,
    /// Camera's serial number
    pub serialNumber: String,
    /// The camera build day e.g. `"build 19110800"`
    pub buildDay: String,
    /// The hardware version e.g. `"IPC_517SD5"`
    pub hardwareVersion: String,
    /// The config version e.g. `"v2.0.0.0"`
    pub cfgVersion: String,
    /// Firmware version usually a combination of config and build versions e.g.
    /// `"v2.0.0.587_19110800"`
    pub firmwareVersion: String,
    /// Unusure possibly a more detailed hardware version e.g. `"IPC_51716M110000000100000"`
    pub detail: String,
}

/// Resolution xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Resolution {
    /// Resolution name is in the format "width*height" i.e. "2304*1296"
    #[yaserde(rename = "resolutionName")]
    pub name: String,
    /// Height of the stream in pixels
    pub width: u32,
    /// Width of the stream in pixels
    pub height: u32,
}

/// Preview xml
///
/// This xml is used to request a stream to start
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Preview {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,

    /// Channel id is usually zero unless using a NVR
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// Handle usually 0 for mainStream and 1 for subStream
    pub handle: u32,
    /// Either `"mainStream"` or `"subStream"`
    #[yaserde(rename = "streamType")]
    pub stream_type: Option<String>,
}

/// Extension xml
///
/// This is used to describe the subsequent payload passed the `payload_offset`
#[derive(PartialEq, Eq, Debug, YaDeserialize, YaSerialize)]
pub struct Extension {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// If the subsequent payload is binary this will be set to 1. Otherwise it is ommited
    #[yaserde(rename = "binaryData")]
    pub binary_data: Option<u32>,
    /// Certain requests such `AbilitySupport` require to know which user this
    /// ability support request is for (why camera dosen't know this based on who
    /// is logged in is unknown... Possible security hole)
    #[yaserde(rename = "userName")]
    pub user_name: Option<String>,
    /// Certain requests such as `AbilitySupport` require details such as what type of
    /// abilities are you intested in. This is a comma seperated list such as
    /// `"system, network, alarm, record, video, image"`
    pub token: Option<String>,
    /// The channel ID. This is usually `0` unless using an NVR
    #[yaserde(rename = "channelId")]
    pub channel_id: Option<u8>,
    /// The rfID used in the PIR
    #[yaserde(rename = "rfId")]
    pub rf_id: Option<u8>,
    /// Encrypted binary has this to verify successful decryption
    #[yaserde(rename = "checkPos")]
    pub check_pos: Option<u32>,
    /// Encrypted binary has this to verify successful decryption
    #[yaserde(rename = "checkValue")]
    pub check_value: Option<u32>,
    /// Used in newer encrypted payload packets
    #[yaserde(rename = "encryptLen")]
    pub encrypt_len: Option<u32>,
}

impl Default for Extension {
    fn default() -> Extension {
        Extension {
            version: xml_ver(),
            binary_data: None,
            user_name: None,
            token: None,
            channel_id: None,
            rf_id: None,
            check_pos: None,
            check_value: None,
            encrypt_len: None,
        }
    }
}

/// SystemGeneral xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct SystemGeneral {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,

    /// Time zone is negative seconds offset from UTC. So +7:00 is -25200
    #[yaserde(rename = "timeZone")]
    pub time_zone: Option<i32>,
    /// Current year
    pub year: Option<i32>,
    /// Current month
    pub month: Option<u8>,
    /// Current day
    pub day: Option<u8>,
    /// Current hour
    pub hour: Option<u8>,
    /// Current minute
    pub minute: Option<u8>,
    /// Current second
    pub second: Option<u8>,

    /// Format to use for On Screen Display usually `"DMY"`
    #[yaserde(rename = "osdFormat")]
    pub osd_format: Option<String>,
    /// Unknown usually `0`
    #[yaserde(rename = "timeFormat")]
    pub time_format: Option<u8>,

    /// Language e.g. `English` will set the language on the reolink app
    pub language: Option<String>,
    /// Name assigned to the camera
    #[yaserde(rename = "deviceName")]
    pub device_name: Option<String>,
}

/// Norm xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Norm {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    // This is usually just `"NTSC"`
    norm: String,
}

/// LedState xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct LedState {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Channel ID of camera to get/set its LED state
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// LED Version, observed value is "2". Should be None when setting the LedState
    #[yaserde(rename = "ledVersion")]
    pub led_version: Option<u32>,
    /// State of the IR LEDs values are "auto", "open", "close"
    pub state: String,
    /// State of the LED status light (blue on light), values are "open", "close"
    #[yaserde(rename = "lightState")]
    pub light_state: String,
}

/// FloodlightStatus xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct FloodlightStatus {
    /// Channel ID of floodlight
    #[yaserde(rename = "channel")]
    pub channel_id: u8,
    /// On or off
    pub status: u8,
}

/// FloodlightStatusList xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct FloodlightStatusList {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// List of events
    #[yaserde(rename = "FloodlightStatus")]
    pub floodlight_status_list: Vec<FloodlightStatus>,
}

/// FloodlightManual xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct FloodlightManual {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Channel ID of floodlight
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// On or off
    pub status: u8,
    /// How long the manual control should apply for
    pub duration: u16,
}

/// rfAlarmCfg xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct RfAlarmCfg {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Rfid
    #[yaserde(rename = "rfID")]
    pub rf_id: u8,
    /// PIR status
    pub enable: u8,
    /// PIR sensitivity
    pub sensitivity: u8,
    /// PIR sensivalue
    pub sensiValue: u8,
    /// reduce False alarm boolean
    pub reduceFalseAlarm: u8,
    /// XML time block for all week days
    #[yaserde(rename = "timeBlockList")]
    pub time_block_list: TimeBlockList,
    /// The alarm handle to attach to this Rf
    #[yaserde(rename = "alarmHandle")]
    pub alarm_handle: AlarmHandle,
}

/// TimeBlockList XML
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
#[yaserde(rename = "timeBlockList")]
pub struct TimeBlockList {
    /// List of time block entries which disable/enable the PIR at a time
    #[yaserde(rename = "timeBlock")]
    pub time_block: Vec<TimeBlock>,
}

/// TimeBlock XML Used to set the time to enable/disable PIR dectection
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
#[yaserde(rename = "timeBlock")]
pub struct TimeBlock {
    /// Whether to enable or disable for this time block
    pub enable: u8,
    /// The day of the week for this block
    pub weekDay: String,
    /// Time to start this block
    #[yaserde(rename = "beginHour")]
    pub begin_hour: u8,
    /// Time to end this block
    #[yaserde(rename = "endHour")]
    pub end_hour: u8,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
/// AlarmHandle Xml
pub struct AlarmHandle {
    /// Items in the alarm handle
    pub item: Vec<AlarmHandleItem>,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
/// An item in the alarm handle
#[yaserde(rename = "item")]
pub struct AlarmHandleItem {
    /// The channel ID
    pub channel: u8,
    /// The handle type: Known values, comma seperated list of snap,rec,push
    #[yaserde(rename = "handleType")]
    pub handle_type: String,
}

/// TalkConfig xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct TalkConfig {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Channel ID of camera to set the TalkConfig
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// Duplex known values `"FDX"`
    pub duplex: String,
    /// audioStreamMode known values `"followVideoStream"`
    #[yaserde(rename = "audioStreamMode")]
    pub audio_stream_mode: String,
    /// AudioConfig contans the details of the audio to follow
    #[yaserde(rename = "audioConfig")]
    pub audio_config: AudioConfig,
}

/// audioConfig xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
#[yaserde(rename = "audioConfig")]
pub struct AudioConfig {
    /// Unknown only sent during TalkAbility request from the camera
    pub priority: Option<u32>,
    /// Audio type known values are `"adpcm"`
    ///
    /// Do not expect camera to support anything else.
    #[yaserde(rename = "audioType")]
    pub audio_type: String,
    /// Audio sample rate known values are `16000`
    #[yaserde(rename = "sampleRate")]
    pub sample_rate: u16,
    /// Precision of data known vaues are `16` (i.e. 16bit)
    #[yaserde(rename = "samplePrecision")]
    pub sample_precision: u16,
    /// Number of audio samples this should be twice the block size for adpcm
    #[yaserde(rename = "lengthPerEncoder")]
    pub length_per_encoder: u16,
    /// Sound track is the number of tracks known values are `"mono"`
    ///
    /// Do not expect camera to support anything else
    #[yaserde(rename = "soundTrack")]
    pub sound_track: String,
}

/// TalkAbility xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct TalkAbility {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Duplexes known values `"FDX"`
    #[yaserde(rename = "duplexList")]
    pub duplex_list: Vec<DuplexList>,
    /// audioStreamModes known values `"followVideoStream"`
    #[yaserde(rename = "audioStreamModeList")]
    pub audio_stream_mode_list: Vec<AudioStreamModeList>,
    /// AudioConfigs contans the details of the audio to follow
    #[yaserde(rename = "audioConfigList")]
    pub audio_config_list: Vec<AudioConfigList>,
}

/// duplexList xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct DuplexList {
    /// The supported duplex known values are "FBX"
    pub duplex: String,
}

/// audioStreamModeList xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct AudioStreamModeList {
    /// The supported audio stream mode
    #[yaserde(rename = "audioStreamMode")]
    pub audio_stream_mode: String,
}

/// audioConfigList xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct AudioConfigList {
    /// The supported audio configs
    #[yaserde(rename = "audioConfig")]
    pub audio_config: AudioConfig,
}

/// An XML that desctibes a list of events such as motion detection
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct AlarmEventList {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// List of events
    #[yaserde(rename = "AlarmEvent")]
    pub alarm_events: Vec<AlarmEvent>,
}

/// An alarm event. Camera can send multiple per message as an array in AlarmEventList.
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct AlarmEvent {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// The channel the event occured on. Usually zero unless from an NVR
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// Motion status. Known values are `"MD"` or `"none"`
    pub status: String,
    /// AI status. Known values are `"people"` or `"none"`
    #[yaserde(rename = "AItype")]
    pub ai_type: Option<String>,
    /// The recording status. Known values `0` or `1`
    pub recording: i32,
    /// The timestamp associated with the recording. `0` if not recording
    #[yaserde(rename = "timeStamp")]
    pub timeStamp: i32,
}

/// The Ptz messages used to move the camera
#[derive(PartialEq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct PtzControl {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// The channel the event occured on. Usually zero unless from an NVR
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// The amount of movement to perform
    pub speed: f32,
    /// The direction to transverse. Known values are `"left"`, `"right"`, `"up"`, `"down"`,
    /// `"leftUp"`, `"leftDown"`, `"rightUp"`, `"rightDown"` and `"stop"`
    pub command: String,
}

/// An XML that describes a list of available PTZ presets
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct PtzPreset {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// The channel ID. Usually zero unless from an NVR
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// List of presets
    #[yaserde(rename = "presetList")]
    pub preset_list: PresetList,
}

/// A preset list
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct PresetList {
    /// List of Presets
    pub preset: Vec<Preset>,
}

/// A preset. Either contains the ID and the name or the ID and the command
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Preset {
    /// The ID of the preset
    pub id: u8,
    /// The preset name
    pub name: Option<String>,
    /// Command: Known values: `"toPos"` and `"setPos"`
    pub command: String,
}

/// A list of battery infos. This message is sent from the camera as
/// an event
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct BatteryList {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Battery info items
    #[yaserde(rename = "BatteryInfo")]
    pub battery_info: Vec<BatteryInfo>,
}

/// The individual battery info
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct BatteryInfo {
    /// The channel the for the camera usually 0
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// Charge status known values, "chargeComplete", "charging", "none",
    #[yaserde(rename = "chargeStatus")]
    pub charge_status: String,
    /// Status of charging port known values: "solarPanel"
    #[yaserde(rename = "adapterStatus")]
    pub adapter_status: String,
    /// Voltage
    pub voltage: i32,
    /// Current
    pub current: i32,
    /// Temperture
    pub temperature: i32,
    /// % charge from 0-100
    #[yaserde(rename = "batteryPercent")]
    pub battery_percent: u32,
    /// Low power flag. Known values 0, 1 (0=false)
    #[yaserde(rename = "lowPower")]
    pub low_power: u32,
    /// Battery version info: Known values 2
    #[yaserde(rename = "batteryVersion")]
    pub battery_version: u32,
}

/// The ability battery info
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct AbilityInfo {
    /// Username with this ability
    #[yaserde(rename = "userName")]
    pub username: String,
    /// System permissions
    pub system: Option<AbilityInfoToken>,
    /// Network permissions
    pub network: Option<AbilityInfoToken>,
    /// Alarm permissions
    pub alarm: Option<AbilityInfoToken>,
    /// Image permissions
    pub image: Option<AbilityInfoToken>,
    /// Video permissions
    pub video: Option<AbilityInfoToken>,
    /// Secutiry permissions
    pub security: Option<AbilityInfoToken>,
    /// Replay permissions
    pub replay: Option<AbilityInfoToken>,
    /// PTZ permissions
    #[yaserde(rename = "PTZ")]
    pub ptz: Option<AbilityInfoToken>,
    /// IO permissions
    #[yaserde(rename = "IO")]
    pub io: Option<AbilityInfoToken>,
    /// Streaming permissions
    pub streaming: Option<AbilityInfoToken>,
}

/// Ability info for system token
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct AbilityInfoToken {
    /// Submodule for this ability info token
    #[yaserde(rename = "subModule")]
    pub sub_module: Vec<AbilityInfoSubModule>,
}

/// Token submodule infomation
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
#[yaserde(rename = "subModule")]
pub struct AbilityInfoSubModule {
    /// The channel the for the camera usually 0
    #[yaserde(rename = "channelId")]
    pub channel_id: Option<u8>,
    /// The comma seperated list of permissions like this: `general_rw, norm_rw, version_ro`
    #[yaserde(rename = "abilityValue")]
    pub ability_value: String,
}

/// PushInfo XML
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct PushInfo {
    /// The token from FCM registration
    pub token: String,
    /// The phone type, known values: `reo_iphone`
    #[yaserde(rename = "phoneType")]
    pub phone_type: String,
    /// A client ID, seems to be an all CAPS MD5 hash of something
    #[yaserde(rename = "clientID")]
    pub client_id: String,
}

/// The Link Type contains the type of connection present
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct LinkType {
    #[yaserde(rename = "type")]
    /// Type of connection known values `"LAN"`
    pub link_type: String,
}

/// The Snap contains the binary jpeg image details
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Snap {
    /// The snap xml version. Observed values "1.1"
    #[yaserde(attribute)]
    pub version: String,
    #[yaserde(rename = "channelId")]
    /// The channel id to get the snapshot from
    pub channel_id: u8,
    /// Unknown, observed values: 0
    /// value is only set on request
    #[yaserde(rename = "logicChannel")]
    pub logic_channel: Option<u8>,
    /// Time of snapshot, zero when requesting
    pub time: u32,
    /// Request a full frame, observed values: 0
    /// value is only set on request
    #[yaserde(rename = "fullFrame")]
    pub full_frame: Option<u32>,
    /// Stream name, observed values: `main`, `sub`
    /// value is only set on request
    #[yaserde(rename = "streamType")]
    pub stream_type: Option<String>,
    /// File name, usually of the form `01_20230518140240.jpg`
    /// value is only set on recieve
    #[yaserde(rename = "fileName")]
    pub file_name: Option<String>,
    /// Size in bytes of the picture
    /// value is only set on recieve
    #[yaserde(rename = "pictureSize")]
    pub picture_size: Option<u32>,
}

/// The primary reply when asked about the stream info
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct StreamInfoList {
    /// The stream infos. There is usually only one of these
    #[yaserde(rename = "StreamInfo")]
    pub stream_infos: Vec<StreamInfo>,
}

/// The individual reply about the stream info
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct StreamInfo {
    /// Bits in the channel number. Observed values `1`
    #[yaserde(rename = "channelBits")]
    pub channel_bits: u32,
    /// List of encode tabeles. These hold the actual stream data
    #[yaserde(rename = "encodeTable")]
    pub encode_tables: Vec<EncodeTable>,
}

/// The individual reply about the stream info
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct EncodeTable {
    /// The internal name of the stream observed values `"mainStream"`, `"subStream"`
    #[yaserde(rename = "type")]
    pub name: String,
    /// The resolution of the stream
    pub resolution: StreamResolution,
    /// The default framerate. This is sometimes an index into the table
    #[yaserde(rename = "defaultFramerate")]
    pub default_framerate: u32,
    /// The default bitrate. This is sometimes an index into the table
    #[yaserde(rename = "defaultBitrate")]
    pub default_bitrate: u32,
    /// Table of valid framerates
    #[yaserde(rename = "framerateTable")]
    pub framerate_table: Vec<u32>,
    /// Table of valid bitrates
    #[yaserde(rename = "bitrateTable")]
    pub bitrate_table: Vec<u32>,
}

/// The resolution of the stream
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct StreamResolution {
    /// Width of the stream
    pub width: u32,
    /// Height of the stream
    pub height: u32,
}

/// Uid xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Uid {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// This the UID of the camera
    pub uid: String,
}

/// FloodlightTask xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct FloodlightTask {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Channel of the camera
    pub channel: u8,
    /// Alarm Mode: Observed values 1
    #[yaserde(rename = "alarmMode")]
    pub alarm_mode: u32,
    /// Enable/Disable floor light on motion
    pub enable: u32,
    /// Last Alarm Mode: Observed values 2
    #[yaserde(rename = "lastAlarmMode")]
    pub last_alarm_mode: u32,
    /// Preview Auto: Observed values 0
    pub preview_auto: u32,
    /// Duration of auto floodlight: Observed values 300 (assume seconds for 5mins)
    pub duration: u32,
    /// Current brightness of floodlight (in %)
    pub brightness_cur: u32,
    /// Max brightness (in %)
    pub brightness_max: Option<u32>,
    /// Min brightness (in %)
    pub brightness_min: Option<u32>,
    /// Schedule fot auto floodlight
    pub schedule: Schedule,
    /// Threshold settings for light sensor to consider nightime
    #[yaserde(rename = "lightSensThreshold")]
    pub light_sens_threshold: LightSensThreshold,
    /// Light of schedled auto floodlights
    #[yaserde(rename = "FloodlightScheduleList")]
    pub floodlight_schedule_list: FloodlightScheduleList,
    /// Some sort of multi brightness
    #[yaserde(rename = "nightLongViewMultiBrightness")]
    pub night_long_view_multi_brightness: NightLongViewMultiBrightness,
    /// Detection Type: Observed values none
    #[yaserde(rename = "detectType")]
    pub detect_type: String,
}

/// Schedule for Floodlight Task
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Schedule {
    /// startHour
    #[yaserde(rename = "startHour")]
    pub start_hour: u32,
    /// startMin: Observed values 0
    #[yaserde(rename = "startMin")]
    pub start_min: Option<u32>,
    /// endHour
    #[yaserde(rename = "endHour")]
    pub end_hour: u32,
    /// endMin: Observed values 0
    #[yaserde(rename = "endMin")]
    pub end_min: Option<u32>,
}

/// Light Sensor Threshold for FloodLightTask
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct LightSensThreshold {
    /// Min: Observed values 1000
    pub min: Option<u32>,
    /// Max: OBserved values 2300
    pub max: Option<u32>,
    /// Light Current Value: Observed Value 1000
    #[yaserde(rename = "lightCur")]
    pub light_cur: u32,
    /// Dark Current Value: Observed Value 1900
    #[yaserde(rename = "darkCur")]
    pub dark_cur: u32,
    /// Light Default: Observed Value 1000
    #[yaserde(rename = "lightDef")]
    pub light_def: Option<u32>,
    /// Dark Default: Observed Value 1900
    #[yaserde(rename = "darkDef")]
    pub dark_def: Option<u32>,
}

/// Floodlight schdule list for FloodlightTask
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct FloodlightScheduleList {
    /// Max Num observed values 32
    #[yaserde(rename = "maxNum")]
    pub max_num: u32,
}

/// NightView Brightness for FloodLightTask
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct NightLongViewMultiBrightness {
    /// Enabled: Observed values 0, 1
    pub enable: u8,
    /// alarmBrightness settings
    #[yaserde(rename = "alarmBrightness")]
    pub alarm_brightness: AlarmBrightness,
    /// alarmDelay settings
    #[yaserde(rename = "alarmDelay")]
    pub alarm_delay: AlarmDelay,
}

/// Alarm brightness for NightLongViewMultiBrightness
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct AlarmBrightness {
    /// Min: Observed values 1
    pub min: Option<u32>,
    /// Max: Observed values 100
    pub max: Option<u32>,
    /// Current: Observed values 100
    pub cur: u32,
    /// Default: Observed values 100
    pub def: Option<u32>,
}

/// Alarm delay for NightLongViewMultiBrightness
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct AlarmDelay {
    /// Min: Observed values 5
    pub min: Option<u32>,
    /// Max: Observed values 600
    pub max: Option<u32>,
    /// Current: Observed values 10
    pub cur: u32,
    /// Default: Observed values 10
    pub def: Option<u32>,
}

/// StartZoomFocus xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct StartZoomFocus {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// Channel ID
    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    /// Command: Observed values: zoomPos
    pub command: String,
    /// Target Position: Observed Values: 2994, 2508, 2888, 3089, 3194, 3163
    #[yaserde(rename = "movePos")]
    pub move_pos: u32,
}

/// Support xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Support {
    /// XML Version
    #[yaserde(attribute)]
    pub version: String,
    /// IO port number (input)
    #[yaserde(rename = "IOInputPortNum")]
    pub io_input_port_num: Option<u32>,
    /// IO port number (output)
    #[yaserde(rename = "IOOutputPortNum")]
    pub io_output_port_num: Option<u32>,
    #[yaserde(rename = "diskNum")]
    /// Number of disks
    pub disk_num: Option<u32>,
    /// Number of video channels
    #[yaserde(rename = "channelNum")]
    pub channel_num: Option<u32>,
    /// Number of audio channels
    #[yaserde(rename = "audioNum")]
    pub audio_num: Option<u32>,
    /// The supported PTZ Mode: pt
    #[yaserde(rename = "ptzMode")]
    pub ptz_mode: Option<String>,
    /// PTZ cfg: 0
    #[yaserde(rename = "ptzCfg")]
    pub ptz_cfg: Option<u32>,
    /// Use b485 ptz
    #[yaserde(rename = "")]
    pub B485: Option<u32>,
    /// Support autoupdate
    #[yaserde(rename = "autoUpdate")]
    pub auto_update: Option<u32>,
    /// Support push notificaion alarms
    #[yaserde(rename = "pushAlarm")]
    pub push_alarm: Option<u32>,
    /// Support ftp
    pub ftp: Option<u32>,
    /// Support test for ftp
    #[yaserde(rename = "ftpTest")]
    pub ftp_test: Option<u32>,
    /// Support email notification
    pub email: Option<u32>,
    /// Support wifi connections
    pub wifi: Option<u32>,
    /// Support recording
    pub record: Option<u32>,
    /// Support test for wifi
    #[yaserde(rename = "wifiTest")]
    pub wifi_test: Option<u32>,
    /// Support rtsp
    pub rtsp: Option<u32>,
    /// Support onvif
    pub onvif: Option<u32>,
    /// Support audio talk
    #[yaserde(rename = "audioTalk")]
    pub audio_talk: Option<u32>,
    /// RF version
    #[yaserde(rename = "rfVersion")]
    pub rf_version: Option<u32>,
    /// Support rtmp
    pub rtmp: Option<u32>,
    /// Has external stream
    #[yaserde(rename = "noExternStream")]
    pub no_extern_stream: Option<u32>,
    /// Time format
    #[yaserde(rename = "timeFormat")]
    pub time_format: Option<u32>,
    /// DDNS version
    #[yaserde(rename = "ddnsVersion")]
    pub ddns_version: Option<u32>,
    /// Email version
    #[yaserde(rename = "emailVersion")]
    pub email_version: Option<u32>,
    /// Push notification version
    #[yaserde(rename = "pushVersion")]
    pub push_version: Option<u32>,
    /// Push notification type: 1
    #[yaserde(rename = "pushType")]
    pub push_type: Option<u32>,
    /// Support audio alarm
    #[yaserde(rename = "audioAlarm")]
    pub audio_alarm: Option<u32>,
    /// Support AP
    #[yaserde(rename = "apMode")]
    pub ap_mode: Option<u32>,
    /// Could version
    #[yaserde(rename = "cloudVersion")]
    pub cloud_version: Option<u32>,
    /// Replay version
    #[yaserde(rename = "replayVersion")]
    pub replay_version: Option<u32>,
    /// mobComVersion
    #[yaserde(rename = "mobComVersion")]
    pub mob_com_version: Option<u32>,
    /// Export images
    #[yaserde(rename = "ExportImport")]
    pub export_import: Option<u32>,
    /// Language version
    #[yaserde(rename = "languageVer")]
    pub language_ver: Option<u32>,
    /// Video standard
    #[yaserde(rename = "videoStandard")]
    pub video_standard: Option<u32>,
    /// Support sync time
    #[yaserde(rename = "syncTime")]
    pub sync_time: Option<u32>,
    /// Support net port
    #[yaserde(rename = "netPort")]
    pub net_port: Option<u32>,
    /// NAS version
    #[yaserde(rename = "nasVersion")]
    pub nas_version: Option<u32>,
    /// Reboot required
    #[yaserde(rename = "needReboot")]
    pub need_reboot: Option<u32>,
    /// Support reboot
    pub reboot: Option<u32>,
    /// Support Audio config
    #[yaserde(rename = "audioCfg")]
    pub audio_cfg: Option<u32>,
    /// Support network diagnosis
    #[yaserde(rename = "networkDiagnosis")]
    pub network_diagnosis: Option<u32>,
    /// Support height adjustment
    #[yaserde(rename = "heightDiffAdjust")]
    pub height_diff_adjust: Option<u32>,
    /// Support upgrade
    pub upgrade: Option<u32>,
    /// Support GPS
    pub gps: Option<u32>,
    /// Support power save config
    #[yaserde(rename = "powerSavingCfg")]
    pub power_saving_cfg: Option<u32>,
    /// Login Locked
    #[yaserde(rename = "loginLocked")]
    pub login_locked: Option<u32>,
    /// View plan
    #[yaserde(rename = "viewPlan")]
    pub view_plan: Option<u32>,
    /// Preview replay limit
    #[yaserde(rename = "previewReplayLimit")]
    pub preview_replay_limit: Option<u32>,
    /// IOT link
    #[yaserde(rename = "IOTLink")]
    pub iot_link: Option<u32>,
    /// IOT link maximum actions
    #[yaserde(rename = "IOTLinkActionMax")]
    pub iot_link_action_max: Option<u32>,
    /// Support record config
    #[yaserde(rename = "recordCfg")]
    pub record_cfg: Option<u32>,
    /// Has large battery
    #[yaserde(rename = "largeBattery")]
    pub large_battery: Option<u32>,
    /// Smart home config
    #[yaserde(rename = "smartHome")]
    pub smart_home: Option<SmartHome>,
    /// Support config for specific channels
    #[yaserde(rename = "item")]
    pub items: Vec<SupportItem>,
}

/// List of smart home items
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct SmartHome {
    /// Versionm
    pub version: u32,
    /// The smarthome items
    #[yaserde(rename = "item")]
    pub items: Vec<SmartHomeItem>,
}

/// Smart home items, are name:version pairs
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct SmartHomeItem {
    /// Name of item: Option<"googleHome">, "amazonAlexa"
    pub name: String,
    /// Version of item: 1
    pub ver: u32,
}

/// Support Items for an individual channel
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct SupportItem {
    /// Channel ID of the item
    #[yaserde(rename = "chnID")]
    pub chn_id: u32,
    /// PTZ type of the channel
    #[yaserde(rename = "ptzType")]
    pub ptz_type: Option<u32>,
    /// RF config
    #[yaserde(rename = "rfCfg")]
    pub rf_cfg: Option<u32>,
    /// Support audio
    #[yaserde(rename = "noAudio")]
    pub no_audio: Option<u32>,
    /// Support auto focus
    #[yaserde(rename = "autoFocus")]
    pub auto_focus: Option<u32>,
    /// Support video clip
    #[yaserde(rename = "videoClip")]
    pub video_clip: Option<u32>,
    /// Has battery
    pub battery: Option<u32>,
    /// ISP config
    #[yaserde(rename = "ispCfg")]
    pub isp_cfg: Option<u32>,
    /// OSD config
    #[yaserde(rename = "osdCfg")]
    pub osd_cfg: Option<u32>,
    /// Support battery analysis
    #[yaserde(rename = "batAnalysis")]
    pub bat_analysis: Option<u32>,
    /// Supports dynamic resolution
    #[yaserde(rename = "dynamicReso")]
    pub dynamic_reso: Option<u32>,
    /// Audio version
    #[yaserde(rename = "audioVersion")]
    pub audio_version: Option<u32>,
    /// Supports LED control
    #[yaserde(rename = "ledCtrl")]
    pub led_ctrl: Option<u32>,
    /// Supports PTZ Control
    #[yaserde(rename = "ptzControl")]
    pub ptz_control: Option<u32>,
    /// Supports new ISP config
    #[yaserde(rename = "newIspCfg")]
    pub new_isp_cfg: Option<u32>,
    /// Supports PTZ presets
    #[yaserde(rename = "ptzPreset")]
    pub ptz_preset: Option<u32>,
    /// Supports PTZ patrol
    #[yaserde(rename = "ptzPatrol")]
    pub ptz_patrol: Option<u32>,
    /// Supports PTZ Tattern
    #[yaserde(rename = "ptzTattern")]
    pub ptz_tattern: Option<u32>,
    /// Supports Auto PT
    #[yaserde(rename = "autoPt")]
    pub auto_pt: Option<u32>,
    /// H264 Profile: 7
    #[yaserde(rename = "h264Profile")]
    pub h264_profile: Option<u32>,
    /// Supports motion alarm
    pub motion: Option<u32>,
    /// AI Type
    #[yaserde(rename = "aitype")]
    pub ai_type: Option<u32>,
    /// Animal AI Type
    #[yaserde(rename = "aiAnimalType")]
    pub ai_animal_type: Option<u32>,
    /// Supports time lapse
    pub timelapse: Option<u32>,
    /// Supports snap
    pub snap: Option<u32>,
    /// Supports encoding control
    #[yaserde(rename = "encCtrl")]
    pub enc_ctrl: Option<u32>,
    /// Has Zoom focus backlash
    #[yaserde(rename = "zfBacklash")]
    pub zf_backlash: Option<u32>,
    /// Supports IOT Link Ability
    #[yaserde(rename = "IOTLinkAbility")]
    pub iot_link_ability: Option<u32>,
    /// Supports IPC audio talk
    #[yaserde(rename = "ipcAudioTalk")]
    pub ipc_audio_talk: Option<u32>,
    /// Supports Bino Config
    #[yaserde(rename = "binoCfg")]
    pub bino_cfg: Option<u32>,
    /// Supports thumbnail
    pub thumbnail: Option<u32>,
}

/// Convience function to return the xml version used throughout the library
pub fn xml_ver() -> String {
    "1.1".to_string()
}

#[test]
fn test_encryption_deser() {
    let sample = indoc!(
        r#"
        <?xml version="1.0" encoding="UTF-8" ?>
        <body>
        <Encryption version="1.1">
        <type>md5</type>
        <nonce>9E6D1FCB9E69846D</nonce>
        </Encryption>
        </body>"#
    );
    let b: BcXml = yaserde::de::from_str(sample).unwrap();
    let enc = b.encryption.as_ref().unwrap();

    assert_eq!(enc.version, "1.1");
    assert_eq!(enc.nonce, "9E6D1FCB9E69846D");
    assert_eq!(enc.type_, "md5");

    let t = BcXml::try_parse(sample.as_bytes()).unwrap();
    match t {
        top_b if top_b == b => {}
        _ => panic!(),
    }
}

#[test]
fn test_login_deser() {
    let sample = indoc!(
        r#"
        <?xml version="1.0" encoding="UTF-8" ?>
        <body>
        <LoginUser version="1.1">
        <userName>9F07915E819A076E2E14169830769D6</userName>
        <password>8EFECD610524A98390F118D2789BE3B</password>
        <userVer>1</userVer>
        </LoginUser>
        <LoginNet version="1.1">
        <type>LAN</type>
        <udpPort>0</udpPort>
        </LoginNet>
        </body>"#
    );
    let b: BcXml = yaserde::de::from_str(sample).unwrap();
    let login_user = b.login_user.unwrap();
    let login_net = b.login_net.unwrap();

    assert_eq!(login_user.version, "1.1");
    assert_eq!(login_user.user_name, "9F07915E819A076E2E14169830769D6");
    assert_eq!(login_user.password, "8EFECD610524A98390F118D2789BE3B");
    assert_eq!(login_user.user_ver, 1);

    assert_eq!(login_net.version, "1.1");
    assert_eq!(login_net.type_, "LAN");
    assert_eq!(login_net.udp_port, 0);
}

#[test]
fn test_login_ser() {
    let sample = indoc!(
        r#"
        <?xml version="1.0" encoding="UTF-8" ?>
        <body>
        <LoginUser version="1.1">
        <userName>9F07915E819A076E2E14169830769D6</userName>
        <password>8EFECD610524A98390F118D2789BE3B</password>
        <userVer>1</userVer>
        </LoginUser>
        <LoginNet version="1.1">
        <type>LAN</type>
        <udpPort>0</udpPort>
        </LoginNet>
        </body>"#
    );

    let b = BcXml {
        login_user: Some(LoginUser {
            version: "1.1".to_string(),
            user_name: "9F07915E819A076E2E14169830769D6".to_string(),
            password: "8EFECD610524A98390F118D2789BE3B".to_string(),
            user_ver: 1,
        }),
        login_net: Some(LoginNet {
            version: "1.1".to_string(),
            type_: "LAN".to_string(),
            udp_port: 0,
        }),
        ..BcXml::default()
    };

    let b2 = BcXml::try_parse(sample.as_bytes()).unwrap();
    let b3 = BcXml::try_parse(b.serialize(vec![]).unwrap().as_slice()).unwrap();

    assert_eq!(b, b2);
    assert_eq!(b, b3);
    assert_eq!(b2, b3);
}

#[test]
fn test_deviceinfo_partial_deser() {
    let sample = indoc!(
        r#"
        <?xml version="1.0" encoding="UTF-8" ?>
        <body>
        <DeviceInfo version="1.1">
        <ipChannel>0</ipChannel>
        <analogChnNum>1</analogChnNum>
        <resolution>
        <resolutionName>3840*2160</resolutionName>
        <width>3840</width>
        <height>2160</height>
        </resolution>
        <language>English</language>
        <sdCard>0</sdCard>
        <ptzMode>none</ptzMode>
        <typeInfo>IPC</typeInfo>
        <softVer>33554880</softVer>
        <B485>0</B485>
        <supportAutoUpdate>0</supportAutoUpdate>
        <userVer>1</userVer>
        </DeviceInfo>
        </body>"#
    );

    // Needs to ignore all the other crap that we don't care about
    let b = BcXml::try_parse(sample.as_bytes()).unwrap();
    match b {
        BcXml {
            device_info:
                Some(DeviceInfo {
                    resolution:
                        Resolution {
                            width: 3840,
                            height: 2160,
                            ..
                        },
                    ..
                }),
            ..
        } => {}
        _ => panic!(),
    }
}

#[test]
fn test_binary_deser() {
    let _ = env_logger::builder().is_test(true).try_init();

    let sample = indoc!(
        r#"
        <?xml version="1.0" encoding="UTF-8" ?>
        <Extension version="1.1">
        <binaryData>1</binaryData>
        </Extension>
    "#
    );
    let b = Extension::try_parse(sample.as_bytes()).unwrap();
    match b {
        Extension {
            binary_data: Some(1),
            ..
        } => {}
        _ => panic!(),
    }
}

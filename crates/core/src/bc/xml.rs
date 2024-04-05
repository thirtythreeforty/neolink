#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use std::{io::BufRead, io::Write};

#[cfg(test)]
use indoc::indoc;

/// There are two types of payloads xml and binary
#[derive(PartialEq, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum BcPayloads {
    /// XML payloads are the more common ones and include payloads for camera controls
    BcXml(BcXml),
    /// Binary payloads are received from the camera for streams and sent to the camera
    /// for talk-back and firmware updates
    Binary(Vec<u8>),
}

/// The top level BC Xml
#[derive(PartialEq, Default, Debug, Deserialize, Serialize)]
#[serde(rename = "body")]
pub struct BcXml {
    /// Encryption xml is received during login and contain the NONCE
    #[serde(rename = "Encryption", skip_serializing_if = "Option::is_none")]
    pub encryption: Option<Encryption>,
    /// LoginUser xml is used during modern login
    #[serde(rename = "LoginUser", skip_serializing_if = "Option::is_none")]
    pub login_user: Option<LoginUser>,
    /// LoginNet xml is used during modern login
    #[serde(rename = "LoginNet", skip_serializing_if = "Option::is_none")]
    pub login_net: Option<LoginNet>,
    /// The final part of a login sequence will return DeviceInfo xml
    #[serde(rename = "DeviceInfo", skip_serializing_if = "Option::is_none")]
    pub device_info: Option<DeviceInfo>,
    /// The VersionInfo xml is recieved in reply to a version request
    #[serde(rename = "VersionInfo", skip_serializing_if = "Option::is_none")]
    pub version_info: Option<VersionInfo>,
    /// Preview xml is used as part of the stream request to set the stream quality and channel
    #[serde(rename = "Preview", skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
    #[serde(rename = "SystemGeneral", skip_serializing_if = "Option::is_none")]
    /// SystemGeneral xml is sent or recieved as part of the clock get/setting
    pub system_general: Option<SystemGeneral>,
    /// Received as part of the Genral system info request
    #[serde(rename = "Norm", skip_serializing_if = "Option::is_none")]
    pub norm: Option<Norm>,
    /// Received as part of the LEDState info request
    #[serde(rename = "LedState", skip_serializing_if = "Option::is_none")]
    pub led_state: Option<LedState>,
    /// Sent as part of the TalkConfig to prepare the camera for audio talk-back
    #[serde(rename = "TalkConfig", skip_serializing_if = "Option::is_none")]
    pub talk_config: Option<TalkConfig>,
    /// rfAlarmCfg xml is sent or recieved as part of the PIR get/setting
    #[serde(rename = "rfAlarmCfg", skip_serializing_if = "Option::is_none")]
    pub rf_alarm_cfg: Option<RfAlarmCfg>,
    /// Revieced as part of the TalkAbility request
    #[serde(rename = "TalkAbility", skip_serializing_if = "Option::is_none")]
    pub talk_ability: Option<TalkAbility>,
    /// Received when motion is detected
    #[serde(rename = "AlarmEventList", skip_serializing_if = "Option::is_none")]
    pub alarm_event_list: Option<AlarmEventList>,
    /// Sent to move the camera
    #[serde(rename = "PtzControl", skip_serializing_if = "Option::is_none")]
    pub ptz_control: Option<PtzControl>,
    /// Sent to manually control the floodlight
    #[serde(rename = "FloodlightManual", skip_serializing_if = "Option::is_none")]
    pub floodlight_manual: Option<FloodlightManual>,
    /// Received when the floodlight status is updated
    #[serde(
        rename = "FloodlightStatusList",
        skip_serializing_if = "Option::is_none"
    )]
    pub floodlight_status_list: Option<FloodlightStatusList>,
    /// Sent or received for the PTZ preset functionality
    #[serde(rename = "PtzPreset", skip_serializing_if = "Option::is_none")]
    pub ptz_preset: Option<PtzPreset>,
    /// Recieved on login/low battery events
    #[serde(rename = "BatteryList", skip_serializing_if = "Option::is_none")]
    pub battery_list: Option<BatteryList>,
    /// Recieved on request for battery info
    #[serde(rename = "BatteryInfo", skip_serializing_if = "Option::is_none")]
    pub battery_info: Option<BatteryInfo>,
    /// Recieved on request for a users persmissions/capabilitoes
    #[serde(rename = "AbilityInfo", skip_serializing_if = "Option::is_none")]
    pub ability_info: Option<AbilityInfo>,
    /// Recieved on request for a users persmissions/capabilitoes
    #[serde(rename = "PushInfo", skip_serializing_if = "Option::is_none")]
    pub push_info: Option<PushInfo>,
    /// Recieved on request for a link type
    #[serde(rename = "LinkType", skip_serializing_if = "Option::is_none")]
    pub link_type: Option<LinkType>,
    /// Recieved AND send for the snap message
    #[serde(rename = "Snap", skip_serializing_if = "Option::is_none")]
    pub snap: Option<Snap>,
    /// The list of streams and their configuration
    #[serde(rename = "StreamInfoList", skip_serializing_if = "Option::is_none")]
    pub stream_info_list: Option<StreamInfoList>,
    /// Thre list of streams and their configuration
    #[serde(rename = "Uid", skip_serializing_if = "Option::is_none")]
    pub uid: Option<Uid>,
    /// The floodlight settings for automatically turning on/off on schedule/motion
    #[serde(rename = "FloodlightTask", skip_serializing_if = "Option::is_none")]
    pub floodlight_task: Option<FloodlightTask>,
    /// For geting the zoom anf focus of the camera
    #[serde(rename = "PtzZoomFocus", skip_serializing_if = "Option::is_none")]
    pub ptz_zoom_focus: Option<PtzZoomFocus>,
    /// For zooming the camera
    #[serde(rename = "StartZoomFocus", skip_serializing_if = "Option::is_none")]
    pub start_zoom_focus: Option<StartZoomFocus>,
    /// Get the support xml
    #[serde(rename = "Support", skip_serializing_if = "Option::is_none")]
    pub support: Option<Support>,
    /// Play a sound
    #[serde(rename = "audioPlayInfo", skip_serializing_if = "Option::is_none")]
    pub audio_play_info: Option<AudioPlayInfo>,
}

impl BcXml {
    pub(crate) fn try_parse(s: impl BufRead) -> Result<Self, quick_xml::de::DeError> {
        quick_xml::de::from_reader(s)
    }
    pub(crate) fn serialize<W: Write>(&self, mut w: W) -> Result<W, quick_xml::de::DeError> {
        let mut writer = quick_xml::writer::Writer::new(&mut w);
        writer
            .write_event(quick_xml::events::Event::Decl(
                quick_xml::events::BytesDecl::new("1.0", Some("UTF-8"), None),
            ))
            .expect("Should be able to serialise a basic xml declaration");
        writer.write_serializable("body", &self)?;
        Ok(w)
    }
}

impl Extension {
    pub(crate) fn try_parse(s: impl BufRead) -> Result<Self, quick_xml::de::DeError> {
        quick_xml::de::from_reader(s)
    }
    pub(crate) fn serialize<W: Write>(&self, mut w: W) -> Result<W, quick_xml::de::DeError> {
        let mut writer = quick_xml::writer::Writer::new(&mut w);
        writer
            .write_event(quick_xml::events::Event::Decl(
                quick_xml::events::BytesDecl::new("1.0", Some("UTF-8"), None),
            ))
            .expect("Should be able to serialise a basic xml declaration");
        writer.write_serializable("Extension", &self)?;
        Ok(w)
    }
}

/// Encryption xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Encryption {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    #[serde(rename = "type")]
    /// The hashing algorithm used. Only observed the value of "md5"
    pub type_: String,
    /// The nonce used to negotiate the login and to generate the AES key
    pub nonce: String,
}

/// LoginUser xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct LoginUser {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Username to login as
    #[serde(rename = "userName")]
    pub user_name: String,
    /// Password for login in plain text
    pub password: String,
    /// Unknown always `1`
    #[serde(rename = "userVer")]
    pub user_ver: u32,
}

/// LoginNet xml
#[derive(PartialEq, Eq, Debug, Deserialize, Serialize)]
pub struct LoginNet {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Type of connection usually LAN (even on wifi)
    #[serde(rename = "type")]
    pub type_: String,
    /// The port for the udp will be `0` for tcp
    #[serde(rename = "udpPort")]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct DeviceInfo {
    /// Version of device info
    #[serde(rename = "@version")]
    pub version: Option<String>,
    /// The resolution xml block
    pub resolution: Resolution,
}

/// VersionInfo xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct VersionInfo {
    /// Name assigned to the camera
    pub name: String,
    /// Model Name
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Resolution {
    /// Resolution name is in the format "width*height" i.e. "2304*1296"
    #[serde(rename = "resolutionName")]
    pub name: String,
    /// Height of the stream in pixels
    pub width: u32,
    /// Width of the stream in pixels
    pub height: u32,
}

/// Preview xml
///
/// This xml is used to request a stream to start
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Preview {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,

    /// Channel id is usually zero unless using a NVR
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// Handle usually 0 for mainStream and 1 for subStream
    pub handle: u32,
    /// Either `"mainStream"` or `"subStream"`
    #[serde(rename = "streamType", skip_serializing_if = "Option::is_none")]
    pub stream_type: Option<String>,
}

/// Extension xml
///
/// This is used to describe the subsequent payload passed the `payload_offset`
#[derive(PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(rename = "Extension")]
pub struct Extension {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// If the subsequent payload is binary this will be set to 1. Otherwise it is ommited
    #[serde(rename = "binaryData", skip_serializing_if = "Option::is_none")]
    pub binary_data: Option<u32>,
    /// Certain requests such `AbilitySupport` require to know which user this
    /// ability support request is for (why camera dosen't know this based on who
    /// is logged in is unknown... Possible security hole)
    #[serde(rename = "userName", skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
    /// Certain requests such as `AbilitySupport` require details such as what type of
    /// abilities are you intested in. This is a comma seperated list such as
    /// `"system, network, alarm, record, video, image"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The channel ID. This is usually `0` unless using an NVR
    #[serde(rename = "channelId", skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<u8>,
    /// The rfID used in the PIR
    #[serde(rename = "rfId", skip_serializing_if = "Option::is_none")]
    pub rf_id: Option<u8>,
    /// Encrypted binary has this to verify successful decryption
    #[serde(rename = "checkPos", skip_serializing_if = "Option::is_none")]
    pub check_pos: Option<u32>,
    /// Encrypted binary has this to verify successful decryption
    #[serde(rename = "checkValue", skip_serializing_if = "Option::is_none")]
    pub check_value: Option<u32>,
    /// Used in newer encrypted payload packets
    #[serde(rename = "encryptLen", skip_serializing_if = "Option::is_none")]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct SystemGeneral {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,

    /// Time zone is negative seconds offset from UTC. So +7:00 is -25200
    #[serde(rename = "timeZone", skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<i32>,
    /// Current year
    #[serde(rename = "year", skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    /// Current month
    #[serde(rename = "month", skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,
    /// Current day
    #[serde(rename = "day", skip_serializing_if = "Option::is_none")]
    pub day: Option<u8>,
    /// Current hour
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hour: Option<u8>,
    /// Current minute
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minute: Option<u8>,
    /// Current second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub second: Option<u8>,

    /// Format to use for On Screen Display usually `"DMY"`
    #[serde(rename = "osdFormat", skip_serializing_if = "Option::is_none")]
    pub osd_format: Option<String>,
    /// Unknown usually `0`
    #[serde(rename = "timeFormat", skip_serializing_if = "Option::is_none")]
    pub time_format: Option<u8>,

    /// Language e.g. `English` will set the language on the reolink app
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Name assigned to the camera
    #[serde(rename = "deviceName", skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

/// Norm xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Norm {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    // This is usually just `"NTSC"`
    norm: String,
}

/// LedState xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct LedState {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Channel ID of camera to get/set its LED state
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// LED Version, observed value is "2". Should be None when setting the LedState
    #[serde(rename = "ledVersion", skip_serializing_if = "Option::is_none")]
    pub led_version: Option<u32>,
    /// State of the IR LEDs values are "auto", "open", "close"
    pub state: String,
    /// State of the LED status light (blue on light), values are "open", "close"
    #[serde(rename = "lightState")]
    pub light_state: String,
}

/// FloodlightStatus xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct FloodlightStatus {
    /// Channel ID of floodlight
    #[serde(rename = "channel")]
    pub channel_id: u8,
    /// On or off
    pub status: u8,
}

/// FloodlightStatusList xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct FloodlightStatusList {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// List of events
    #[serde(rename = "FloodlightStatus")]
    pub floodlight_status_list: Vec<FloodlightStatus>,
}

/// FloodlightManual xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct FloodlightManual {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Channel ID of floodlight
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// On or off
    pub status: u8,
    /// How long the manual control should apply for
    pub duration: u16,
}

/// rfAlarmCfg xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct RfAlarmCfg {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Rfid
    #[serde(rename = "rfID")]
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
    #[serde(rename = "timeBlockList")]
    pub time_block_list: TimeBlockList,
    /// The alarm handle to attach to this Rf
    #[serde(rename = "alarmHandle")]
    pub alarm_handle: AlarmHandle,
}

/// TimeBlockList XML
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
#[serde(rename = "timeBlockList")]
pub struct TimeBlockList {
    /// List of time block entries which disable/enable the PIR at a time
    #[serde(rename = "timeBlock")]
    pub time_block: Vec<TimeBlock>,
}

/// TimeBlock XML Used to set the time to enable/disable PIR dectection
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
#[serde(rename = "timeBlock")]
pub struct TimeBlock {
    /// Whether to enable or disable for this time block
    pub enable: u8,
    /// The day of the week for this block
    pub weekDay: String,
    /// Time to start this block
    #[serde(rename = "beginHour")]
    pub begin_hour: u8,
    /// Time to end this block
    #[serde(rename = "endHour")]
    pub end_hour: u8,
}

#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
/// AlarmHandle Xml
pub struct AlarmHandle {
    /// Items in the alarm handle
    pub item: Vec<AlarmHandleItem>,
}

#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
/// An item in the alarm handle
#[serde(rename = "item")]
pub struct AlarmHandleItem {
    /// The channel ID
    pub channel: u8,
    /// The handle type: Known values, comma seperated list of snap,rec,push
    #[serde(rename = "handleType")]
    pub handle_type: String,
}

/// TalkConfig xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct TalkConfig {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Channel ID of camera to set the TalkConfig
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// Duplex known values `"FDX"`
    pub duplex: String,
    /// audioStreamMode known values `"followVideoStream"`
    #[serde(rename = "audioStreamMode")]
    pub audio_stream_mode: String,
    /// AudioConfig contans the details of the audio to follow
    #[serde(rename = "audioConfig")]
    pub audio_config: AudioConfig,
}

/// audioConfig xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
#[serde(rename = "audioConfig")]
pub struct AudioConfig {
    /// Unknown only sent during TalkAbility request from the camera
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
    /// Audio type known values are `"adpcm"`
    ///
    /// Do not expect camera to support anything else.
    #[serde(rename = "audioType")]
    pub audio_type: String,
    /// Audio sample rate known values are `16000`
    #[serde(rename = "sampleRate")]
    pub sample_rate: u16,
    /// Precision of data known vaues are `16` (i.e. 16bit)
    #[serde(rename = "samplePrecision")]
    pub sample_precision: u16,
    /// Number of audio samples this should be twice the block size for adpcm
    #[serde(rename = "lengthPerEncoder")]
    pub length_per_encoder: u16,
    /// Sound track is the number of tracks known values are `"mono"`
    ///
    /// Do not expect camera to support anything else
    #[serde(rename = "soundTrack")]
    pub sound_track: String,
}

/// TalkAbility xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct TalkAbility {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Duplexes known values `"FDX"`
    #[serde(rename = "duplexList")]
    pub duplex_list: Vec<DuplexList>,
    /// audioStreamModes known values `"followVideoStream"`
    #[serde(rename = "audioStreamModeList")]
    pub audio_stream_mode_list: Vec<AudioStreamModeList>,
    /// AudioConfigs contans the details of the audio to follow
    #[serde(rename = "audioConfigList")]
    pub audio_config_list: Vec<AudioConfigList>,
}

/// duplexList xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct DuplexList {
    /// The supported duplex known values are "FBX"
    pub duplex: String,
}

/// audioStreamModeList xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AudioStreamModeList {
    /// The supported audio stream mode
    #[serde(rename = "audioStreamMode")]
    pub audio_stream_mode: String,
}

/// audioConfigList xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AudioConfigList {
    /// The supported audio configs
    #[serde(rename = "audioConfig")]
    pub audio_config: AudioConfig,
}

/// An XML that desctibes a list of events such as motion detection
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AlarmEventList {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// List of events
    #[serde(rename = "AlarmEvent")]
    pub alarm_events: Vec<AlarmEvent>,
}

/// An alarm event. Camera can send multiple per message as an array in AlarmEventList.
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AlarmEvent {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// The channel the event occured on. Usually zero unless from an NVR
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// Motion status. Known values are `"MD"` or `"none"`
    pub status: String,
    /// AI status. Known values are `"people"` or `"none"`
    #[serde(rename = "AItype", skip_serializing_if = "Option::is_none")]
    pub ai_type: Option<String>,
    /// The recording status. Known values `0` or `1`
    pub recording: i32,
    /// The timestamp associated with the recording. `0` if not recording
    #[serde(rename = "timeStamp")]
    pub timeStamp: i32,
}

/// The Ptz messages used to move the camera
#[derive(PartialEq, Default, Debug, Deserialize, Serialize)]
pub struct PtzControl {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// The channel the event occured on. Usually zero unless from an NVR
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// The amount of movement to perform
    pub speed: f32,
    /// The direction to transverse. Known values are `"left"`, `"right"`, `"up"`, `"down"`,
    /// `"leftUp"`, `"leftDown"`, `"rightUp"`, `"rightDown"` and `"stop"`
    pub command: String,
}

/// An XML that describes a list of available PTZ presets
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct PtzPreset {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// The channel ID. Usually zero unless from an NVR
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// List of presets
    #[serde(rename = "presetList")]
    pub preset_list: PresetList,
}

/// A preset list
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct PresetList {
    /// List of Presets
    pub preset: Vec<Preset>,
}

/// A preset. Either contains the ID and the name or the ID and the command
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Preset {
    /// The ID of the preset
    pub id: u8,
    /// The preset name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Command: Known values: `"toPos"` and `"setPos"`
    pub command: String,
}

/// A list of battery infos. This message is sent from the camera as
/// an event
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct BatteryList {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Battery info items
    #[serde(rename = "BatteryInfo")]
    pub battery_info: Vec<BatteryInfo>,
}

/// The individual battery info
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct BatteryInfo {
    /// The channel the for the camera usually 0
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// Charge status known values, "chargeComplete", "charging", "none",
    #[serde(rename = "chargeStatus")]
    pub charge_status: String,
    /// Status of charging port known values: "solarPanel"
    #[serde(rename = "adapterStatus")]
    pub adapter_status: String,
    /// Voltage
    pub voltage: i32,
    /// Current
    pub current: i32,
    /// Temperture
    pub temperature: i32,
    /// % charge from 0-100
    #[serde(rename = "batteryPercent")]
    pub battery_percent: u32,
    /// Low power flag. Known values 0, 1 (0=false)
    #[serde(rename = "lowPower")]
    pub low_power: u32,
    /// Battery version info: Known values 2
    #[serde(rename = "batteryVersion")]
    pub battery_version: u32,
}

/// The ability battery info
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AbilityInfo {
    /// Username with this ability
    #[serde(rename = "userName")]
    pub username: String,
    /// System permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<AbilityInfoToken>,
    /// Network permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<AbilityInfoToken>,
    /// Alarm permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alarm: Option<AbilityInfoToken>,
    /// Image permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<AbilityInfoToken>,
    /// Video permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<AbilityInfoToken>,
    /// Secutiry permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<AbilityInfoToken>,
    /// Replay permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replay: Option<AbilityInfoToken>,
    /// PTZ permissions
    #[serde(rename = "PTZ", skip_serializing_if = "Option::is_none")]
    pub ptz: Option<AbilityInfoToken>,
    /// IO permissions
    #[serde(rename = "IO", skip_serializing_if = "Option::is_none")]
    pub io: Option<AbilityInfoToken>,
    /// Streaming permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<AbilityInfoToken>,
}

/// Ability info for system token
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AbilityInfoToken {
    /// Submodule for this ability info token
    #[serde(rename = "subModule")]
    pub sub_module: Vec<AbilityInfoSubModule>,
}

/// Token submodule infomation
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
#[serde(rename = "subModule")]
pub struct AbilityInfoSubModule {
    /// The channel the for the camera usually 0
    #[serde(rename = "channelId", skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<u8>,
    /// The comma seperated list of permissions like this: `general_rw, norm_rw, version_ro`
    #[serde(rename = "abilityValue")]
    pub ability_value: String,
}

/// PushInfo XML
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct PushInfo {
    /// The token from FCM registration
    pub token: String,
    /// The phone type, known values: `reo_iphone`
    #[serde(rename = "phoneType")]
    pub phone_type: String,
    /// A client ID, seems to be an all CAPS MD5 hash of something
    #[serde(rename = "clientID")]
    pub client_id: String,
}

/// The Link Type contains the type of connection present
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct LinkType {
    #[serde(rename = "type")]
    /// Type of connection known values `"LAN"`
    pub link_type: String,
}

/// The Snap contains the binary jpeg image details
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Snap {
    /// The snap xml version. Observed values "1.1"
    #[serde(rename = "@version")]
    pub version: String,
    #[serde(rename = "channelId")]
    /// The channel id to get the snapshot from
    pub channel_id: u8,
    /// Unknown, observed values: 0
    /// value is only set on request
    #[serde(rename = "logicChannel", skip_serializing_if = "Option::is_none")]
    pub logic_channel: Option<u8>,
    /// Time of snapshot, zero when requesting
    pub time: u32,
    /// Request a full frame, observed values: 0
    /// value is only set on request
    #[serde(rename = "fullFrame", skip_serializing_if = "Option::is_none")]
    pub full_frame: Option<u32>,
    /// Stream name, observed values: `main`, `sub`
    /// value is only set on request
    #[serde(rename = "streamType", skip_serializing_if = "Option::is_none")]
    pub stream_type: Option<String>,
    /// File name, usually of the form `01_20230518140240.jpg`
    /// value is only set on recieve
    #[serde(rename = "fileName", skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// Size in bytes of the picture
    /// value is only set on recieve
    #[serde(rename = "pictureSize", skip_serializing_if = "Option::is_none")]
    pub picture_size: Option<u32>,
}

/// The primary reply when asked about the stream info
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct StreamInfoList {
    /// The stream infos. There is usually only one of these
    #[serde(rename = "StreamInfo")]
    pub stream_infos: Vec<StreamInfo>,
}

/// The individual reply about the stream info
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct StreamInfo {
    /// Bits in the channel number. Observed values `1`
    #[serde(rename = "channelBits")]
    pub channel_bits: u32,
    /// List of encode tabeles. These hold the actual stream data
    #[serde(rename = "encodeTable")]
    pub encode_tables: Vec<EncodeTable>,
}

/// The individual reply about the stream info
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct EncodeTable {
    /// The internal name of the stream observed values `"mainStream"`, `"subStream"`
    #[serde(rename = "type")]
    pub name: String,
    /// The resolution of the stream
    pub resolution: StreamResolution,
    /// The default framerate. This is sometimes an index into the table
    #[serde(rename = "defaultFramerate")]
    pub default_framerate: u32,
    /// The default bitrate. This is sometimes an index into the table
    #[serde(rename = "defaultBitrate")]
    pub default_bitrate: u32,
    /// Table of valid framerates
    #[serde(rename = "framerateTable")]
    pub framerate_table: String,
    /// Table of valid bitrates
    #[serde(rename = "bitrateTable")]
    pub bitrate_table: String,
}

/// The resolution of the stream
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct StreamResolution {
    /// Width of the stream
    pub width: u32,
    /// Height of the stream
    pub height: u32,
}

/// Uid xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Uid {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// This the UID of the camera
    pub uid: String,
}

/// FloodlightTask xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct FloodlightTask {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Channel of the camera
    pub channel: u8,
    /// Alarm Mode: Observed values 1
    #[serde(rename = "alarmMode")]
    pub alarm_mode: u32,
    /// Enable/Disable floor light on motion
    pub enable: u32,
    /// Last Alarm Mode: Observed values 2
    #[serde(rename = "lastAlarmMode")]
    pub last_alarm_mode: u32,
    /// Preview Auto: Observed values 0
    pub preview_auto: u32,
    /// Duration of auto floodlight: Observed values 300 (assume seconds for 5mins)
    pub duration: u32,
    /// Current brightness of floodlight (in %)
    pub brightness_cur: u32,
    /// Max brightness (in %)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness_max: Option<u32>,
    /// Min brightness (in %)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness_min: Option<u32>,
    /// Schedule fot auto floodlight
    pub schedule: Schedule,
    /// Threshold settings for light sensor to consider nightime
    #[serde(rename = "lightSensThreshold")]
    pub light_sens_threshold: LightSensThreshold,
    /// Light of schedled auto floodlights
    #[serde(rename = "FloodlightScheduleList")]
    pub floodlight_schedule_list: FloodlightScheduleList,
    /// Some sort of multi brightness
    #[serde(rename = "nightLongViewMultiBrightness")]
    pub night_long_view_multi_brightness: NightLongViewMultiBrightness,
    /// Detection Type: Observed values none
    #[serde(rename = "detectType")]
    pub detect_type: String,
}

/// Schedule for Floodlight Task
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Schedule {
    /// startHour
    #[serde(rename = "startHour")]
    pub start_hour: u32,
    /// startMin: Observed values 0
    #[serde(rename = "startMin", skip_serializing_if = "Option::is_none")]
    pub start_min: Option<u32>,
    /// endHour
    #[serde(rename = "endHour")]
    pub end_hour: u32,
    /// endMin: Observed values 0
    #[serde(rename = "endMin", skip_serializing_if = "Option::is_none")]
    pub end_min: Option<u32>,
}

/// Light Sensor Threshold for FloodLightTask
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct LightSensThreshold {
    /// Min: Observed values 1000
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    /// Max: OBserved values 2300
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    /// Light Current Value: Observed Value 1000
    #[serde(rename = "lightCur")]
    pub light_cur: u32,
    /// Dark Current Value: Observed Value 1900
    #[serde(rename = "darkCur")]
    pub dark_cur: u32,
    /// Light Default: Observed Value 1000
    #[serde(rename = "lightDef", skip_serializing_if = "Option::is_none")]
    pub light_def: Option<u32>,
    /// Dark Default: Observed Value 1900
    #[serde(rename = "darkDef", skip_serializing_if = "Option::is_none")]
    pub dark_def: Option<u32>,
}

/// Floodlight schdule list for FloodlightTask
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct FloodlightScheduleList {
    /// Max Num observed values 32
    #[serde(rename = "maxNum")]
    pub max_num: u32,
}

/// NightView Brightness for FloodLightTask
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct NightLongViewMultiBrightness {
    /// Enabled: Observed values 0, 1
    pub enable: u8,
    /// alarmBrightness settings
    #[serde(rename = "alarmBrightness")]
    pub alarm_brightness: AlarmBrightness,
    /// alarmDelay settings
    #[serde(rename = "alarmDelay")]
    pub alarm_delay: AlarmDelay,
}

/// Alarm brightness for NightLongViewMultiBrightness
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AlarmBrightness {
    /// Min: Observed values 1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    /// Max: Observed values 100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    /// Current: Observed values 100
    pub cur: u32,
    /// Default: Observed values 100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub def: Option<u32>,
}

/// Alarm delay for NightLongViewMultiBrightness
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AlarmDelay {
    /// Min: Observed values 5
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    /// Max: Observed values 600
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    /// Current: Observed values 10
    pub cur: u32,
    /// Default: Observed values 10
    #[serde(skip_serializing_if = "Option::is_none")]
    pub def: Option<u32>,
}

/// PtzZoomFocus xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct PtzZoomFocus {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Channel ID
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// Max, min and current zoom. (Read Only)
    pub zoom: HelperPosition,
    /// Max, min and current focus. (Read Only)
    pub focus: HelperPosition,
}

/// StartZoomFocus xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct StartZoomFocus {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// Channel ID
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// Command: Observed values: zoomPos. (Write Only)
    pub command: String,
    /// Target Position: Observed Values: 2994, 2508, 2888, 3089, 3194, 3163. (Write Only)
    #[serde(rename = "movePos")]
    pub move_pos: u32,
}

/// Helper for Max, Min, Curr pos of zoom/focus
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct HelperPosition {
    /// Max value
    #[serde(rename = "maxPos")]
    pub max_pos: u32,
    /// Min value
    #[serde(rename = "minPos")]
    pub min_pos: u32,
    /// Curr value
    #[serde(rename = "curPos")]
    pub cur_pos: u32,
}

/// Support xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct Support {
    /// XML Version
    #[serde(rename = "@version")]
    pub version: String,
    /// IO port number (input)
    #[serde(rename = "IOInputPortNum", skip_serializing_if = "Option::is_none")]
    pub io_input_port_num: Option<u32>,
    /// IO port number (output)
    #[serde(rename = "IOOutputPortNum", skip_serializing_if = "Option::is_none")]
    pub io_output_port_num: Option<u32>,
    #[serde(rename = "diskNum")]
    /// Number of disks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_num: Option<u32>,
    /// Number of video channels
    #[serde(rename = "channelNum", skip_serializing_if = "Option::is_none")]
    pub channel_num: Option<u32>,
    /// Number of audio channels
    #[serde(rename = "audioNum", skip_serializing_if = "Option::is_none")]
    pub audio_num: Option<u32>,
    /// The supported PTZ Mode: pt
    #[serde(rename = "ptzMode", skip_serializing_if = "Option::is_none")]
    pub ptz_mode: Option<String>,
    /// PTZ cfg: 0
    #[serde(rename = "ptzCfg", skip_serializing_if = "Option::is_none")]
    pub ptz_cfg: Option<u32>,
    /// Use b485 ptz
    #[serde(rename = "b485", skip_serializing_if = "Option::is_none")]
    pub B485: Option<u32>,
    /// Support autoupdate
    #[serde(rename = "autoUpdate", skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<u32>,
    /// Support push notificaion alarms
    #[serde(rename = "pushAlarm", skip_serializing_if = "Option::is_none")]
    pub push_alarm: Option<u32>,
    /// Support ftp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ftp: Option<u32>,
    /// Support test for ftp
    #[serde(rename = "ftpTest", skip_serializing_if = "Option::is_none")]
    pub ftp_test: Option<u32>,
    /// Support email notification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<u32>,
    /// Support wifi connections
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wifi: Option<u32>,
    /// Support recording
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<u32>,
    /// Support test for wifi
    #[serde(rename = "wifiTest", skip_serializing_if = "Option::is_none")]
    pub wifi_test: Option<u32>,
    /// Support rtsp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtsp: Option<u32>,
    /// Support onvif
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onvif: Option<u32>,
    /// Support audio talk
    #[serde(rename = "audioTalk", skip_serializing_if = "Option::is_none")]
    pub audio_talk: Option<u32>,
    /// RF version
    #[serde(rename = "rfVersion", skip_serializing_if = "Option::is_none")]
    pub rf_version: Option<u32>,
    /// Support rtmp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtmp: Option<u32>,
    /// Has external stream
    #[serde(rename = "noExternStream", skip_serializing_if = "Option::is_none")]
    pub no_extern_stream: Option<u32>,
    /// Time format
    #[serde(rename = "timeFormat", skip_serializing_if = "Option::is_none")]
    pub time_format: Option<u32>,
    /// DDNS version
    #[serde(rename = "ddnsVersion", skip_serializing_if = "Option::is_none")]
    pub ddns_version: Option<u32>,
    /// Email version
    #[serde(rename = "emailVersion", skip_serializing_if = "Option::is_none")]
    pub email_version: Option<u32>,
    /// Push notification version
    #[serde(rename = "pushVersion", skip_serializing_if = "Option::is_none")]
    pub push_version: Option<u32>,
    /// Push notification type: 1
    #[serde(rename = "pushType", skip_serializing_if = "Option::is_none")]
    pub push_type: Option<u32>,
    /// Support audio alarm
    #[serde(rename = "audioAlarm", skip_serializing_if = "Option::is_none")]
    pub audio_alarm: Option<u32>,
    /// Support AP
    #[serde(rename = "apMode", skip_serializing_if = "Option::is_none")]
    pub ap_mode: Option<u32>,
    /// Could version
    #[serde(rename = "cloudVersion", skip_serializing_if = "Option::is_none")]
    pub cloud_version: Option<u32>,
    /// Replay version
    #[serde(rename = "replayVersion", skip_serializing_if = "Option::is_none")]
    pub replay_version: Option<u32>,
    /// mobComVersion
    #[serde(rename = "mobComVersion", skip_serializing_if = "Option::is_none")]
    pub mob_com_version: Option<u32>,
    /// Export images
    #[serde(rename = "ExportImport", skip_serializing_if = "Option::is_none")]
    pub export_import: Option<u32>,
    /// Language version
    #[serde(rename = "languageVer", skip_serializing_if = "Option::is_none")]
    pub language_ver: Option<u32>,
    /// Video standard
    #[serde(rename = "videoStandard", skip_serializing_if = "Option::is_none")]
    pub video_standard: Option<u32>,
    /// Support sync time
    #[serde(rename = "syncTime", skip_serializing_if = "Option::is_none")]
    pub sync_time: Option<u32>,
    /// Support net port
    #[serde(rename = "netPort", skip_serializing_if = "Option::is_none")]
    pub net_port: Option<u32>,
    /// NAS version
    #[serde(rename = "nasVersion", skip_serializing_if = "Option::is_none")]
    pub nas_version: Option<u32>,
    /// Reboot required
    #[serde(rename = "needReboot", skip_serializing_if = "Option::is_none")]
    pub need_reboot: Option<u32>,
    /// Support reboot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reboot: Option<u32>,
    /// Support Audio config
    #[serde(rename = "audioCfg", skip_serializing_if = "Option::is_none")]
    pub audio_cfg: Option<u32>,
    /// Support network diagnosis
    #[serde(rename = "networkDiagnosis", skip_serializing_if = "Option::is_none")]
    pub network_diagnosis: Option<u32>,
    /// Support height adjustment
    #[serde(rename = "heightDiffAdjust", skip_serializing_if = "Option::is_none")]
    pub height_diff_adjust: Option<u32>,
    /// Support upgrade
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<u32>,
    /// Support GPS
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gps: Option<u32>,
    /// Support power save config
    #[serde(rename = "powerSavingCfg", skip_serializing_if = "Option::is_none")]
    pub power_saving_cfg: Option<u32>,
    /// Login Locked
    #[serde(rename = "loginLocked", skip_serializing_if = "Option::is_none")]
    pub login_locked: Option<u32>,
    /// View plan
    #[serde(rename = "viewPlan", skip_serializing_if = "Option::is_none")]
    pub view_plan: Option<u32>,
    /// Preview replay limit
    #[serde(rename = "previewReplayLimit", skip_serializing_if = "Option::is_none")]
    pub preview_replay_limit: Option<u32>,
    /// IOT link
    #[serde(rename = "IOTLink", skip_serializing_if = "Option::is_none")]
    pub iot_link: Option<u32>,
    /// IOT link maximum actions
    #[serde(rename = "IOTLinkActionMax", skip_serializing_if = "Option::is_none")]
    pub iot_link_action_max: Option<u32>,
    /// Support record config
    #[serde(rename = "recordCfg", skip_serializing_if = "Option::is_none")]
    pub record_cfg: Option<u32>,
    /// Has large battery
    #[serde(rename = "largeBattery", skip_serializing_if = "Option::is_none")]
    pub large_battery: Option<u32>,
    /// Smart home config
    #[serde(rename = "smartHome", skip_serializing_if = "Option::is_none")]
    pub smart_home: Option<SmartHome>,
    /// Support config for specific channels
    #[serde(rename = "item")]
    pub items: Vec<SupportItem>,
}

/// List of smart home items
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct SmartHome {
    /// Versionm
    pub version: u32,
    /// The smarthome items
    #[serde(rename = "item")]
    pub items: Vec<SmartHomeItem>,
}

/// Smart home items, are name:version pairs
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct SmartHomeItem {
    /// Name of item: Option<"googleHome">, "amazonAlexa"
    pub name: String,
    /// Version of item: 1
    pub ver: u32,
}

/// Support Items for an individual channel
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct SupportItem {
    /// Channel ID of the item
    #[serde(rename = "chnID")]
    pub chn_id: u32,
    /// PTZ type of the channel
    #[serde(rename = "ptzType", skip_serializing_if = "Option::is_none")]
    pub ptz_type: Option<u32>,
    /// RF config
    #[serde(rename = "rfCfg", skip_serializing_if = "Option::is_none")]
    pub rf_cfg: Option<u32>,
    /// Support audio
    #[serde(rename = "noAudio", skip_serializing_if = "Option::is_none")]
    pub no_audio: Option<u32>,
    /// Support auto focus
    #[serde(rename = "autoFocus", skip_serializing_if = "Option::is_none")]
    pub auto_focus: Option<u32>,
    /// Support video clip
    #[serde(rename = "videoClip", skip_serializing_if = "Option::is_none")]
    pub video_clip: Option<u32>,
    /// Has battery
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<u32>,
    /// ISP config
    #[serde(rename = "ispCfg", skip_serializing_if = "Option::is_none")]
    pub isp_cfg: Option<u32>,
    /// OSD config
    #[serde(rename = "osdCfg", skip_serializing_if = "Option::is_none")]
    pub osd_cfg: Option<u32>,
    /// Support battery analysis
    #[serde(rename = "batAnalysis", skip_serializing_if = "Option::is_none")]
    pub bat_analysis: Option<u32>,
    /// Supports dynamic resolution
    #[serde(rename = "dynamicReso", skip_serializing_if = "Option::is_none")]
    pub dynamic_reso: Option<u32>,
    /// Audio version
    #[serde(rename = "audioVersion", skip_serializing_if = "Option::is_none")]
    pub audio_version: Option<u32>,
    /// Supports LED control
    #[serde(rename = "ledCtrl", skip_serializing_if = "Option::is_none")]
    pub led_ctrl: Option<u32>,
    /// Supports PTZ Control
    #[serde(rename = "ptzControl", skip_serializing_if = "Option::is_none")]
    pub ptz_control: Option<u32>,
    /// Supports new ISP config
    #[serde(rename = "newIspCfg", skip_serializing_if = "Option::is_none")]
    pub new_isp_cfg: Option<u32>,
    /// Supports PTZ presets
    #[serde(rename = "ptzPreset", skip_serializing_if = "Option::is_none")]
    pub ptz_preset: Option<u32>,
    /// Supports PTZ patrol
    #[serde(rename = "ptzPatrol", skip_serializing_if = "Option::is_none")]
    pub ptz_patrol: Option<u32>,
    /// Supports PTZ Tattern
    #[serde(rename = "ptzTattern", skip_serializing_if = "Option::is_none")]
    pub ptz_tattern: Option<u32>,
    /// Supports Auto PT
    #[serde(rename = "autoPt", skip_serializing_if = "Option::is_none")]
    pub auto_pt: Option<u32>,
    /// H264 Profile: 7
    #[serde(rename = "h264Profile", skip_serializing_if = "Option::is_none")]
    pub h264_profile: Option<u32>,
    /// Supports motion alarm
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<u32>,
    /// AI Type
    #[serde(rename = "aitype", skip_serializing_if = "Option::is_none")]
    pub ai_type: Option<u32>,
    /// Animal AI Type
    #[serde(rename = "aiAnimalType", skip_serializing_if = "Option::is_none")]
    pub ai_animal_type: Option<u32>,
    /// Supports time lapse
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timelapse: Option<u32>,
    /// Supports snap
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap: Option<u32>,
    /// Supports encoding control
    #[serde(rename = "encCtrl", skip_serializing_if = "Option::is_none")]
    pub enc_ctrl: Option<u32>,
    /// Has Zoom focus backlash
    #[serde(rename = "zfBacklash", skip_serializing_if = "Option::is_none")]
    pub zf_backlash: Option<u32>,
    /// Supports IOT Link Ability
    #[serde(rename = "IOTLinkAbility", skip_serializing_if = "Option::is_none")]
    pub iot_link_ability: Option<u32>,
    /// Supports IPC audio talk
    #[serde(rename = "ipcAudioTalk", skip_serializing_if = "Option::is_none")]
    pub ipc_audio_talk: Option<u32>,
    /// Supports Bino Config
    #[serde(rename = "binoCfg", skip_serializing_if = "Option::is_none")]
    pub bino_cfg: Option<u32>,
    /// Supports thumbnail
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<u32>,
}

/// Instruct camera to play an audio alarm, usually this is the siren
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize)]
pub struct AudioPlayInfo {
    /// Channel ID
    #[serde(rename = "channelId")]
    pub channel_id: u8,
    /// Playmode: 0
    #[serde(rename = "playMode")]
    pub play_mode: u32,
    /// Duration: 0
    #[serde(rename = "playDuration")]
    pub play_duration: u32,
    /// Times to play: 1
    #[serde(rename = "playTimes")]
    pub play_times: u32,
    /// On or Off: 0
    #[serde(rename = "onOff")]
    pub on_off: u32,
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
    let b: BcXml = quick_xml::de::from_str(sample).unwrap();
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
    let b: BcXml = quick_xml::de::from_str(sample).unwrap();
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
    let b3 = BcXml::try_parse(b.serialize(vec![]).unwrap().as_ref()).unwrap();
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
        <firmVersion>00000000000000</firmVersion>
        <IOInputPortNum>0</IOInputPortNum>
        <IOOutputPortNum>0</IOOutputPortNum>
        <diskNum>0</diskNum>
        <type>ipc</type>
        <channelNum>1</channelNum>
        <audioNum>1</audioNum>
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
        <hardVer>0</hardVer>
        <panelVer>0</panelVer>
        <hdChannel1>0</hdChannel1>
        <hdChannel2>0</hdChannel2>
        <hdChannel3>0</hdChannel3>
        <hdChannel4>0</hdChannel4>
        <norm>NTSC</norm>
        <osdFormat>YMD</osdFormat>
        <B485>0</B485>
        <supportAutoUpdate>0</supportAutoUpdate>
        <userVer>1</userVer>
        </DeviceInfo>
        <StreamInfoList version="1.1">
        <StreamInfo>
        <channelBits>1</channelBits>
        <encodeTable>
        <type>mainStream</type>
        <resolution>
        <width>3840</width>
        <height>2160</height>
        </resolution>
        <defaultFramerate>20</defaultFramerate>
        <defaultBitrate>6144</defaultBitrate>
        <framerateTable>20,18,16,15,12,10,8,6,4,2</framerateTable>
        <bitrateTable>4096,5120,6144,7168,8192</bitrateTable>
        </encodeTable>
        <encodeTable>
        <type>subStream</type>
        <resolution>
        <width>640</width>
        <height>360</height>
        </resolution>
        <defaultFramerate>7</defaultFramerate>
        <defaultBitrate>160</defaultBitrate>
        <framerateTable>15,10,7,4</framerateTable>
        <bitrateTable>64,128,160,192,256,384,512</bitrateTable>
        </encodeTable>
        </StreamInfo>
        <StreamInfo>
        <channelBits>1</channelBits>
        <encodeTable>
        <type>mainStream</type>
        <resolution>
        <width>2560</width>
        <height>1440</height>
        </resolution>
        <defaultFramerate>25</defaultFramerate>
        <defaultBitrate>0</defaultBitrate>
        <framerateTable>25,22,20,18,16,15,12,10,8,6,4,2</framerateTable>
        <bitrateTable>1024,1536,2048,3072,4096,5120,6144,7168,8192</bitrateTable>
        </encodeTable>
        <encodeTable>
        <type>subStream</type>
        <resolution>
        <width>640</width>
        <height>360</height>
        </resolution>
        <defaultFramerate>7</defaultFramerate>
        <defaultBitrate>160</defaultBitrate>
        <framerateTable>15,10,7,4</framerateTable>
        <bitrateTable>64,128,160,192,256,384,512</bitrateTable>
        </encodeTable>
        </StreamInfo>
        <StreamInfo>
        <channelBits>1</channelBits>
        <encodeTable>
        <type>mainStream</type>
        <resolution>
        <width>2304</width>
        <height>1296</height>
        </resolution>
        <defaultFramerate>25</defaultFramerate>
        <defaultBitrate>0</defaultBitrate>
        <framerateTable>25,22,20,18,16,15,12,10,8,6,4,2</framerateTable>
        <bitrateTable>1024,1536,2048,3072,4096,5120,6144,7168,8192</bitrateTable>
        </encodeTable>
        <encodeTable>
        <type>subStream</type>
        <resolution>
        <width>640</width>
        <height>360</height>
        </resolution>
        <defaultFramerate>7</defaultFramerate>
        <defaultBitrate>160</defaultBitrate>
        <framerateTable>15,10,7,4</framerateTable>
        <bitrateTable>64,128,160,192,256,384,512</bitrateTable>
        </encodeTable>
        </StreamInfo>
        </StreamInfoList>
        </body>
"#
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

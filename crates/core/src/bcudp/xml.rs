// YaSerde currently macro-expands names like __type__value from type_

use std::io::{Read, Write};
// YaSerde is currently naming the traits and the derive macros identically
use yaserde::{ser::Config, YaDeserialize, YaSerialize};
use yaserde_derive::{YaDeserialize, YaSerialize};

/// The top level of the UDP xml is P2P
#[derive(PartialEq, Eq, Debug, YaDeserialize, YaSerialize)]
#[yaserde(rename = "P2P")]
pub struct UdpXml {
    /// C2D_S xml
    #[yaserde(rename = "C2D_S")]
    pub start: Option<DiscoveryStartAny>,
    /// C2D_S xml
    #[yaserde(rename = "C2D_C")]
    pub start_with: Option<DiscoveryStartWith>,
    /// D2C_T xml
    #[yaserde(rename = "D2C_T")]
    pub camera_transmission: Option<CameraTransmission>,
    /// C2D_T xml
    #[yaserde(rename = "C2D_T")]
    pub client_transmission: Option<ClientTransmission>,
    /// D2C_CFM xml
    #[yaserde(rename = "D2C_CFM")]
    pub camera_cfm: Option<CameraCfm>,
    /// C2D_DISC xml
    #[yaserde(rename = "C2D_DISC")]
    pub disconnect: Option<Disconnect>,
}

impl UdpXml {
    pub(crate) fn try_parse(s: impl Read) -> Result<Self, String> {
        yaserde::de::from_reader(s)
    }
    pub(crate) fn serialize<W: Write>(&self, w: W) -> Result<W, String> {
        yaserde::ser::serialize_with_writer(self, w, &Config::default())
    }
}

/// C2D_S xml
///
/// The camera will send binary data to port 3000
/// to whoever it gets this message from
///
/// It should be broadcasted to port 2015
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct DiscoveryStartAny {
    /// The destination to reply to
    pub to: StartTo,
}

/// to xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct StartTo {
    /// Port to open udp connections with
    pub port: u32,
}

/// C2D_C xml
///
/// This will start a connection with any camera that has this UID
/// It should be broadcasted to port 2018
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct DiscoveryStartWith {
    /// UID of the camera the client wants to connect with
    pub uid: String,
    /// Cli contains the udp port to communicate on
    pub cli: StartCli,
    /// The cid is the client ID
    pub cid: u32,
    /// Maximum transmission size,
    pub mtu: u32,
    /// Debug mode. Purpose unknown
    pub debug: bool,
    /// Os of the machine known values are `"MAC"`
    #[yaserde(rename = "p")]
    pub os: String,
}

/// C2D_C xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct StartCli {
    /// Port to start udp communication with
    pub port: u32,
}

/// C2D_DISC xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Disconnect {
    /// The client connection ID
    pub cid: u32,
    /// The camera connection ID
    pub did: u32,
}

/// D2C_T xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct CameraTransmission {
    /// The camera SID
    pub sid: u32,
    /// Type of connection observed values are `"local"`
    pub conn: String,
    /// The client connection ID
    pub cid: u32,
    /// The camera connection ID
    pub did: u32,
}

/// C2D_T xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct ClientTransmission {
    /// The camera SID
    pub sid: u32,
    /// Type of connection observed values are `"local"`
    pub conn: String,
    /// The client connection ID
    pub cid: u32,
    /// Maximum size in bytes of a transmission
    pub mtu: u32,
}

/// D2C_CFM xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct CameraCfm {
    /// The camera SID
    pub sid: u32,
    /// Type of connection observed values are `"local"`
    pub conn: String,
    /// Unknown known values are `0`
    pub rsp: u32,
    /// The client connection ID
    pub cid: u32,
    /// The camera connection ID
    pub did: u32,
    /// The time but only value that has been observed is `0
    pub time_r: u32,
}

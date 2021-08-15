// YaSerde currently macro-expands names like __type__value from type_

use std::io::{Read, Write};
// YaSerde is currently naming the traits and the derive macros identically
use yaserde::{ser::Config, YaDeserialize, YaSerialize};
use yaserde_derive::{YaDeserialize, YaSerialize};

/// The top level of the UDP xml is P2P
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
#[yaserde(rename = "P2P")]
pub struct UdpXml {
    /// C2D_S xml Discovery of any client
    #[yaserde(rename = "C2D_S")]
    pub c2d_s: Option<C2dS>,
    /// C2D_S xml Discovery of client with a UID
    #[yaserde(rename = "C2D_C")]
    pub c2d_c: Option<C2dC>,
    /// D2C_C_C xml Reply from discovery
    #[yaserde(rename = "D2C_C_R")]
    pub d2c_c_r: Option<D2cCr>,
    /// D2C_T xml
    #[yaserde(rename = "D2C_T")]
    pub d2c_t: Option<D2cT>,
    /// C2D_T xml
    #[yaserde(rename = "C2D_T")]
    pub c2d_t: Option<C2dT>,
    /// D2C_CFM xml
    #[yaserde(rename = "D2C_CFM")]
    pub d2c_cfm: Option<D2cCfm>,
    /// C2D_DISC xml Disconnect
    #[yaserde(rename = "C2D_DISC")]
    pub c2d_disc: Option<C2dDisc>,
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
pub struct C2dS {
    /// The destination to reply to
    pub to: PortList,
}

/// Port list xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct PortList {
    /// Port to open udp connections with
    pub port: u32,
}

/// C2D_C xml
///
/// This will start a connection with any camera that has this UID
/// It should be broadcasted to port 2018
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct C2dC {
    /// UID of the camera the client wants to connect with
    pub uid: String,
    /// Cli contains the udp port to communicate on
    pub cli: ClientList,
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

/// Client List xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct ClientList {
    /// Port to start udp communication with
    pub port: u32,
}

/// D2C_C_R xml
///
/// This will start a connection with any camera that has this UID
/// It should be broadcasted to port 2018
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct D2cCr {
    /// Called timer but not sure what it is a timer of
    pub timer: Timer,
    /// Unknown
    pub rsp: u32,
    /// Client ID
    pub cid: u32,
    /// Camera ID
    pub did: u32,
}

/// Timer provided by D2C_C_R
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Timer {
    /// Unknown
    def: u32,
    /// Unknown
    hb: u32,
    /// Unknown
    hbt: u32,
}

/// C2D_DISC xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct C2dDisc {
    /// The client connection ID
    pub cid: u32,
    /// The camera connection ID
    pub did: u32,
}

/// D2C_T xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct D2cT {
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
pub struct C2dT {
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
pub struct D2cCfm {
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

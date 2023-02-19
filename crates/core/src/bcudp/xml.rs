// YaSerde currently macro-expands names like __type__value from type_

use std::io::{Read, Write};
// YaSerde is currently naming the traits and the derive macros identically
use yaserde::{ser::Config, YaDeserialize, YaSerialize};
use yaserde_derive::{YaDeserialize, YaSerialize};

/// The top level of the UDP xml is P2P
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
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
    /// D2C_DISC xml Disconnect
    #[yaserde(rename = "D2C_DISC")]
    pub d2c_disc: Option<D2cDisc>,
    /// R2C_DISC xml Disconnect
    #[yaserde(rename = "R2C_DISC")]
    pub r2c_disc: Option<R2cDisc>,
    /// C2M_Q xml client to middle man query
    #[yaserde(rename = "C2M_Q")]
    pub c2m_q: Option<C2mQ>,
    /// M2C_Q_R xml middle man to client query reply
    #[yaserde(rename = "M2C_Q_R")]
    pub m2c_q_r: Option<M2cQr>,
    /// C2R_C xml client to register connect
    #[yaserde(rename = "C2R_C")]
    pub c2r_c: Option<C2rC>,
    /// R2C_T xml register to clinet with device ID etc
    #[yaserde(rename = "R2C_T")]
    pub r2c_t: Option<R2cT>,
    /// R2C_T xml register to clinet with device ID etc handled over dmap ONLY
    #[yaserde(rename = "R2C_C_R")]
    pub r2c_c_r: Option<R2cCr>,
    /// C2R_CFM xml client to register CFM
    #[yaserde(rename = "C2R_CFM")]
    pub c2r_cfm: Option<C2rCfm>,
    /// C2D_A xml client to device accept
    #[yaserde(rename = "C2D_A")]
    pub c2d_a: Option<C2dA>,
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
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct C2dS {
    /// The destination to reply to
    pub to: PortList,
}

/// Port list xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct PortList {
    /// Port to open udp connections with
    pub port: u32,
}

/// C2D_C xml
///
/// This will start a connection with any camera that has this UID
/// It should be broadcasted to port 2018
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct C2dC {
    /// UID of the camera the client wants to connect with
    pub uid: String,
    /// Cli contains the udp port to communicate on
    pub cli: ClientList,
    /// The cid is the client ID
    pub cid: i32,
    /// Maximum transmission size,
    pub mtu: u32,
    /// Debug mode. Purpose unknown
    pub debug: bool,
    /// Os of the machine known values are `"MAC"`, `"WIN"`
    #[yaserde(rename = "p")]
    pub os: String,
}

/// Client List xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct ClientList {
    /// Port to start udp communication with
    pub port: u32,
}

/// D2C_C_R xml
///
/// This will start a connection with any camera that has this UID
/// It should be broadcasted to port 2018
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct D2cCr {
    /// Called timer but not sure what it is a timer of
    pub timer: Timer,
    /// Unknown
    pub rsp: u32,
    /// Client ID
    pub cid: i32,
    /// Camera ID
    pub did: i32,
}

/// Timer provided by D2C_C_R
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct Timer {
    /// Unknown
    def: u32,
    /// Unknown
    hb: u32,
    /// Unknown
    hbt: u32,
}

/// C2D_DISC xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct C2dDisc {
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
}

/// D2C_DISC xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct D2cDisc {
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
}

/// R2C_DISC xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct R2cDisc {
    /// The sid
    pub sid: u32,
}

/// D2C_T xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct D2cT {
    /// The camera SID
    pub sid: u32,
    /// Type of connection observed values are `"local"` `"relay"`, `"map"`
    pub conn: String,
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
}

/// C2D_T xml
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct C2dT {
    /// The camera SID
    pub sid: u32,
    /// Type of connection observed values are `"local"`
    pub conn: String,
    /// The client connection ID
    pub cid: i32,
    /// Maximum size in bytes of a transmission
    pub mtu: u32,
}

/// C2M_Q xml
///
/// This is from client to a reolink middle man server
///
/// It should be sent to a reolink p2p sever on port 9999
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct C2mQ {
    /// UID to look up
    pub uid: String,
    /// Os of the machine known values are `"MAC"`, `"WIN"`
    #[yaserde(rename = "p")]
    pub os: String,
}

/// M2C_Q_R xml
///
/// This is from middle man reolink server to client
///
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct M2cQr {
    /// The register server location
    pub reg: IpPort,
    /// The relay server location
    pub relay: IpPort,
    /// The log server location
    pub log: IpPort,
    /// The camera location
    pub t: IpPort,
}

/// Used as part of M2C_Q_R to provide the host and port
///
/// of the register, relay and log servers
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct IpPort {
    /// Ip of the service
    pub ip: String,
    /// Port of the service
    pub port: u16,
}

/// C2R_C xml
///
/// This is from client to the register reolink server
///
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct C2rC {
    /// The UID to register connecition request with
    pub uid: String,
    /// The location of the client
    pub cli: IpPort,
    /// The location of the relay server
    pub relay: IpPort,
    /// The client id
    pub cid: i32,
    /// Debug setting. Unknown purpose observed values are `0`
    pub debug: bool,
    /// Inet family. Observed values `4`
    pub family: u8,
    /// Os of the machine known values are `"MAC"`, `"WIN"`
    #[yaserde(rename = "p")]
    pub os: String,
    /// The revision. Known values None and 3
    #[yaserde(rename = "r")]
    pub revision: Option<i32>,
}

/// R2C_T xml
///
/// This is from register reolink server to clinet with device ip and did etc
///
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct R2cT {
    /// The location of the camera
    pub dmap: Option<IpPort>,
    /// The location of the camera
    pub dev: Option<IpPort>,
    /// The client id
    pub cid: i32,
    /// The camera SID
    pub sid: u32,
}

/// R2C_C_R xml
///
/// This is from register reolink server to clinet with device ip and did etc
/// during a relay
///
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct R2cCr {
    /// Dmap camera location
    pub dmap: IpPort,
    /// The location of the relay
    pub relay: IpPort,
    /// The nat type. Known values `"NULL"`
    pub nat: String,
    /// The camera SID
    pub sid: u32,
    /// rsp. Known values `0`
    pub rsp: i32,
    /// ac. Known values. `127536491`
    pub ac: u32,
}

/// D2C_CFM xml
///
/// Device to client, with connection started from middle man server
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct D2cCfm {
    /// The camera SID
    pub sid: u32,
    /// Type of connection observed values are `"local"`
    pub conn: String,
    /// Unknown known values are `0`
    pub rsp: u32,
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
    /// The time but only value that has been observed is `0
    pub time_r: u32,
}

/// C2R_CFM xml
///
/// Client to register
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct C2rCfm {
    /// The camera SID
    pub sid: u32,
    /// Type of connection observed values are `"local"`
    pub conn: String,
    /// Unknown known values are `0`
    pub rsp: u32,
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
}

/// C2D_A xml
///
/// Client to device accept.
/// Sent it reply to a D2C_T
#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize, Clone)]
pub struct C2dA {
    /// The camera SID
    pub sid: u32,
    /// Type of connection observed values are `"local"`
    pub conn: String,
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
    /// Maximum size in bytes of a transmission
    pub mtu: u32,
}

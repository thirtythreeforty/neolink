use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::io::Write;

/// The top level of the UDP xml is P2P
#[derive(PartialEq, Eq, Debug, Deserialize, Serialize, Clone)]
#[serde(rename = "P2P")]
pub enum UdpXml {
    /// C2D_S xml Discovery of any client
    #[serde(rename = "C2D_S")]
    C2dS(C2dS),
    /// C2D_S xml Discovery of client with a UID
    #[serde(rename = "C2D_C")]
    C2dC(C2dC),
    /// D2C_C_C xml Reply from discovery
    #[serde(rename = "D2C_C_R")]
    D2cCr(D2cCr),
    /// D2C_T xml
    #[serde(rename = "D2C_T")]
    D2cT(D2cT),
    /// C2D_T xml
    #[serde(rename = "C2D_T")]
    C2dT(C2dT),
    /// D2C_CFM xml
    #[serde(rename = "D2C_CFM")]
    D2cCfm(D2cCfm),
    /// C2D_DISC xml Disconnect
    #[serde(rename = "C2D_DISC")]
    C2dDisc(C2dDisc),
    /// D2C_DISC xml Disconnect
    #[serde(rename = "D2C_DISC")]
    D2cDisc(D2cDisc),
    /// R2C_DISC xml Disconnect
    #[serde(rename = "R2C_DISC")]
    R2cDisc(R2cDisc),
    /// C2M_Q xml client to middle man query
    #[serde(rename = "C2M_Q")]
    C2mQ(C2mQ),
    /// M2C_Q_R xml middle man to client query reply
    #[serde(rename = "M2C_Q_R")]
    M2cQr(M2cQr),
    /// C2R_C xml client to register connect
    #[serde(rename = "C2R_C")]
    C2rC(C2rC),
    /// R2C_T xml register to clinet with device ID etc
    #[serde(rename = "R2C_T")]
    R2cT(R2cT),
    /// R2C_T xml register to clinet with device ID etc handled over dmap ONLY
    #[serde(rename = "R2C_C_R")]
    R2cCr(R2cCr),
    /// C2R_CFM xml client to register CFM
    #[serde(rename = "C2R_CFM")]
    C2rCfm(C2rCfm),
    /// C2D_A xml client to device accept
    #[serde(rename = "C2D_A")]
    C2dA(C2dA),
    /// C2D_HB xml client to device heartbeat. This is the keep alive
    #[serde(rename = "C2D_HB")]
    C2dHb(C2dHb),
    /// C2D_HB xml client to device heartbeat. This is the keep alive
    #[serde(rename = "C2R_HB")]
    C2rHb(C2rHb),
}

/// The top level holder for P2P we auto add/remove this at serde
#[derive(PartialEq, Eq, Debug, Deserialize, Serialize, Clone)]
struct P2P {
    #[serde(rename = "$value")]
    xml: UdpXml,
}

impl UdpXml {
    pub(crate) fn try_parse(s: impl BufRead) -> Result<Self, quick_xml::de::DeError> {
        let p2p: Result<P2P, _> = quick_xml::de::from_reader(s);
        p2p.map(|i| i.xml)
    }
    pub(crate) fn serialize<W: Write>(&self, mut w: W) -> Result<W, quick_xml::de::DeError> {
        let mut writer = quick_xml::writer::Writer::new(&mut w);
        // No header on a UdpXml
        // writer.write_event(quick_xml::events::Event::Decl(
        //     quick_xml::events::BytesDecl::new("1.0", Some("UTF-8"), None),
        // ))?;
        writer
            .create_element("P2P")
            .write_inner_content::<_, quick_xml::de::DeError>(|writer| {
                writer.write_serializable("", &self)?;
                Ok(())
            })?;

        Ok(w)
    }
}

/// C2D_S xml
///
/// The camera will send binary data to port 3000
/// to whoever it gets this message from
///
/// It should be broadcasted to port 2015
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct C2dS {
    /// The destination to reply to
    pub to: PortList,
}

/// Port list xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct PortList {
    /// Port to open udp connections with
    pub port: u32,
}

/// C2D_C xml
///
/// This will start a connection with any camera that has this UID
/// It should be broadcasted to port 2018
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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
    #[serde(rename = "p")]
    pub os: String,
}

/// Client List xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct ClientList {
    /// Port to start udp communication with
    pub port: u32,
}

/// D2C_C_R xml
///
/// This will start a connection with any camera that has this UID
/// It should be broadcasted to port 2018
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct Timer {
    /// Unknown
    def: u32,
    /// Unknown
    hb: u32,
    /// Unknown
    hbt: u32,
}

/// C2D_DISC xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct C2dDisc {
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
}

/// D2C_DISC xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct D2cDisc {
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
}

/// R2C_DISC xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct R2cDisc {
    /// The sid
    pub sid: u32,
}

/// D2C_T xml
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct C2mQ {
    /// UID to look up
    pub uid: String,
    /// Os of the machine known values are `"MAC"`, `"WIN"`
    #[serde(rename = "p")]
    pub os: String,
}

/// M2C_Q_R xml
///
/// This is from middle man reolink server to client
///
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct IpPort {
    /// Ip of the service
    pub ip: String,
    /// Port of the service
    pub port: u16,
}

impl std::convert::TryFrom<IpPort> for std::net::SocketAddr {
    type Error = crate::Error;

    fn try_from(src: IpPort) -> Result<Self, Self::Error> {
        Ok(src
            .ip
            .parse::<std::net::IpAddr>()
            .map(|ip| std::net::SocketAddr::new(ip, src.port))?)
    }
}

/// C2R_C xml
///
/// This is from client to the register reolink server
///
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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
    #[serde(rename = "p")]
    pub os: String,
    /// The revision. Known values None and 3
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    pub revision: Option<i32>,
}

/// R2C_T xml
///
/// This is from register reolink server to clinet with device ip and did etc
///
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct R2cT {
    /// The location of the camera
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dmap: Option<IpPort>,
    /// The location of the camera
    #[serde(skip_serializing_if = "Option::is_none")]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct R2cCr {
    /// Dev camera location (actual local ip)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev: Option<IpPort>,
    /// Dmap camera location
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dmap: Option<IpPort>,
    /// The location of the relay
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay: Option<IpPort>,
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
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

/// C2D_HB xml
///
/// Client to device heart beat.
/// Seems to act as a keep alive
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct C2dHb {
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
}

/// C2R_HB xml
///
/// Client to device heart beat.
/// Seems to act as a keep alive
#[derive(PartialEq, Eq, Default, Debug, Deserialize, Serialize, Clone)]
pub struct C2rHb {
    /// The connection ID
    pub sid: u32,
    /// The client connection ID
    pub cid: i32,
    /// The camera connection ID
    pub did: i32,
}

// YaSerde currently macro-expands names like __type__value from type_
#![allow(non_snake_case)]

use std::io::{Read, Write};
// YaSerde is currently naming the traits and the derive macros identically
use yaserde::{ser::Config, YaDeserialize, YaSerialize};
use yaserde_derive::{YaDeserialize, YaSerialize};

#[cfg(test)]
use indoc::indoc;

#[derive(PartialEq, Eq, Debug, YaDeserialize)]
#[yaserde(flatten)]
pub enum BcPayloads {
    #[yaserde(rename = "body")]
    BcXml(BcXml),
    #[yaserde(flatten)]
    Binary(Vec<u8>),
}

// Required for YaDeserialize
impl Default for BcPayloads {
    fn default() -> Self {
        BcPayloads::Binary(Default::default())
    }
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
#[yaserde(rename = "body")]
pub struct BcXml {
    #[yaserde(rename = "Encryption")]
    pub encryption: Option<Encryption>,
    #[yaserde(rename = "LoginUser")]
    pub login_user: Option<LoginUser>,
    #[yaserde(rename = "LoginNet")]
    pub login_net: Option<LoginNet>,
    #[yaserde(rename = "DeviceInfo")]
    pub device_info: Option<DeviceInfo>,
    #[yaserde(rename = "VersionInfo")]
    pub version_info: Option<VersionInfo>,
    #[yaserde(rename = "Preview")]
    pub preview: Option<Preview>,
    #[yaserde(rename = "SystemGeneral")]
    pub system_general: Option<SystemGeneral>,
    #[yaserde(rename = "Norm")]
    pub norm: Option<Norm>,
}

impl BcXml {
    pub fn try_parse(s: impl Read) -> Result<Self, String> {
        yaserde::de::from_reader(s)
    }
    pub fn serialize<W: Write>(&self, w: W) -> Result<W, String> {
        yaserde::ser::serialize_with_writer(self, w, &Config::default())
    }
}

impl Extension {
    pub fn try_parse(s: impl Read) -> Result<Self, String> {
        yaserde::de::from_reader(s)
    }
    pub fn serialize<W: Write>(&self, w: W) -> Result<W, String> {
        yaserde::ser::serialize_with_writer(self, w, &Config::default())
    }
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Encryption {
    #[yaserde(attribute)]
    pub version: String,
    #[yaserde(rename = "type")]
    pub type_: String,
    pub nonce: String,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct LoginUser {
    #[yaserde(attribute)]
    pub version: String,
    #[yaserde(rename = "userName")]
    pub user_name: String,
    pub password: String,
    #[yaserde(rename = "userVer")]
    pub user_ver: u32,
}

#[derive(PartialEq, Eq, Debug, YaDeserialize, YaSerialize)]
pub struct LoginNet {
    #[yaserde(attribute)]
    pub version: String,
    #[yaserde(rename = "type")]
    pub type_: String,
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

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct DeviceInfo {
    pub resolution: Resolution,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct VersionInfo {
    pub name: String,
    pub serialNumber: String,
    pub buildDay: String,
    pub hardwareVersion: String,
    pub cfgVersion: String,
    pub firmwareVersion: String,
    pub detail: String,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Resolution {
    #[yaserde(rename = "resolutionName")]
    pub name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Preview {
    #[yaserde(attribute)]
    pub version: String,

    #[yaserde(rename = "channelId")]
    pub channel_id: u8,
    pub handle: u32,
    #[yaserde(rename = "streamType")]
    pub stream_type: String,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Extension {
    #[yaserde(attribute)]
    pub version: String,
    #[yaserde(rename = "binaryData")]
    pub binary_data: Option<u32>,
    #[yaserde(rename = "userName")]
    pub user_name: Option<String>,
    pub token: Option<String>,
    #[yaserde(rename = "channelId")]
    pub channel_id: Option<u8>,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct SystemGeneral {
    #[yaserde(attribute)]
    pub version: String,

    #[yaserde(rename = "timeZone")]
    pub time_zone: Option<i32>,
    pub year: Option<i32>,
    pub month: Option<u8>,
    pub day: Option<u8>,
    pub hour: Option<u8>,
    pub minute: Option<u8>,
    pub second: Option<u8>,

    #[yaserde(rename = "osdFormat")]
    pub osd_format: Option<String>,
    #[yaserde(rename = "timeFormat")]
    pub time_format: Option<u8>,

    pub language: Option<String>,
    #[yaserde(rename = "deviceName")]
    pub device_name: Option<String>,
}

#[derive(PartialEq, Eq, Default, Debug, YaDeserialize, YaSerialize)]
pub struct Norm {
    #[yaserde(attribute)]
    pub version: String,
    norm: String,
}

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
        top_b if top_b == b => assert!(true),
        _ => assert!(false),
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
        } => assert!(true),
        _ => assert!(false),
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
        } => assert!(true),
        _ => assert!(false),
    }
}

use serde::Deserialize;
use std::fmt;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub cameras: Vec<CameraConfig>,
}

#[derive(Debug, Deserialize)]
pub struct CameraConfig {
    pub name: String,

    #[serde(rename="address")]
    pub camera_addr: SocketAddr,

    #[serde(rename="serve")]
    pub bind_addr: BindAddr,

    pub stream: Option<String>,
    pub username: String,
    pub password: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct BindAddr {
    host: Option<String>,
    port: u16,
}

impl fmt::Display for BindAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref host) = self.host {
            write!(f, "{}:{}", host, self.port)
        } else {
            write!(f, "port {}", self.port)
        }
    }
}

impl ToSocketAddrs for BindAddr {
    type Iter = <(&'static str, u16) as ToSocketAddrs>::Iter;
    fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
        let host = self.host.as_deref().unwrap_or("0.0.0.0");
        (host, self.port).to_socket_addrs()
    }
}

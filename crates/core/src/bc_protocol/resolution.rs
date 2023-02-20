//! This is a helper module to resolve either to a UID or a SockerAddr

use log::*;
use std::{
    io::Error,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs},
};

/// Select permitted discovery methods
///
/// This is used for UID lookup, it is unused with
/// TPC/known ip address cameras
#[derive(Debug, Copy, Clone)]
pub enum DiscoveryMethods {
    /// Forbid all discovery methods. Only TCP connections with known addresses will work
    None,
    /// Allow local discovery on the local network using broadcasts
    /// This method does NOT contact reolink servers
    Local,
    /// Allow contact with the reolink servers to learn the ip address but DO NOT
    /// allow the camera/clinet to communicate through the reolink servers.
    ///
    /// **This also enabled `Local` discovery**
    Remote,
    /// Allow contact with the reolink servers to learn the ip address and relay the connection
    /// through those servers.
    ///
    /// **This also enabled `Local` and `Remote` discovery**
    Relay,
}

/// Used to return either the SocketAddr or the UID
pub enum SocketAddrOrUid {
    /// When the result is a addr it will be this
    SocketAddr(SocketAddr),
    /// When the result is a UID
    Uid(String, DiscoveryMethods),
}

/// An extension of ToSocketAddrs that will also resolve to a camera UID
pub trait ToSocketAddrsOrUid: ToSocketAddrs {
    /// The return type of the function
    type UidIter: Iterator<Item = SocketAddrOrUid>;

    /// This handles the actual resolution. It should first check the
    /// normal [.to_socket_addrs()] and if that fails it should check
    /// if it looks like a uid
    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error>;
}

impl ToSocketAddrsOrUid for SocketAddr {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl ToSocketAddrsOrUid for str {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        match self.to_socket_addrs() {
            Ok(addrs) => Ok(addrs
                .map(SocketAddrOrUid::SocketAddr)
                .collect::<Vec<_>>()
                .into_iter()),
            Err(e) => {
                debug!("Trying as uid");
                let re = regex::Regex::new(r"^[0-9A-Za-z]+$").unwrap();
                if re.is_match(self) {
                    Ok(vec![SocketAddrOrUid::Uid(
                        self.to_string(),
                        DiscoveryMethods::Local,
                    )]
                    .into_iter())
                } else {
                    debug!("Regex fails {:?}  => {:?} ", re, self);
                    Err(e)
                }
            }
        }
    }
}

impl ToSocketAddrsOrUid for String {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        match self.to_socket_addrs() {
            Ok(addrs) => Ok(addrs
                .map(SocketAddrOrUid::SocketAddr)
                .collect::<Vec<_>>()
                .into_iter()),
            Err(e) => {
                debug!("Trying as uid");
                let re = regex::Regex::new(r"^[0-9A-Za-z]+$").unwrap();
                if re.is_match(self) {
                    Ok(vec![SocketAddrOrUid::Uid(
                        self.to_string(),
                        DiscoveryMethods::Local,
                    )]
                    .into_iter())
                } else {
                    debug!("Regex fails {:?}  => {:?} ", re, self);
                    Err(e)
                }
            }
        }
    }
}

impl ToSocketAddrsOrUid for (&str, u16) {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl ToSocketAddrsOrUid for (IpAddr, u16) {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl ToSocketAddrsOrUid for (String, u16) {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl ToSocketAddrsOrUid for (Ipv4Addr, u16) {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl ToSocketAddrsOrUid for (Ipv6Addr, u16) {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl ToSocketAddrsOrUid for SocketAddrV4 {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl ToSocketAddrsOrUid for SocketAddrV6 {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl<'a> ToSocketAddrsOrUid for &'a [SocketAddr] {
    type UidIter = std::vec::IntoIter<SocketAddrOrUid>;

    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        Ok(self
            .to_socket_addrs()?
            .map(SocketAddrOrUid::SocketAddr)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl<T: ToSocketAddrsOrUid + ?Sized> ToSocketAddrsOrUid for &T {
    type UidIter = T::UidIter;
    fn to_socket_addrs_or_uid(&self) -> Result<Self::UidIter, Error> {
        (**self).to_socket_addrs_or_uid()
    }
}

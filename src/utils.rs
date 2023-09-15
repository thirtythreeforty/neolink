//! Contains code that is not specific to any of the subcommands
//!
use log::*;

use super::config::CameraConfig;
use anyhow::{anyhow, Context, Error, Result};
use neolink_core::bc_protocol::{
    BcCamera, BcCameraOpt, ConnectionProtocol, Credentials, DiscoveryMethods, MaxEncryption,
};
use std::{
    fmt::{Display, Error as FmtError, Formatter},
    net::{IpAddr, ToSocketAddrs},
    str::FromStr,
};

pub(crate) fn timeout<F>(future: F) -> tokio::time::Timeout<F>
where
    F: std::future::Future,
{
    tokio::time::timeout(tokio::time::Duration::from_secs(15), future)
}

pub(crate) enum AddressOrUid {
    Address(String),
    Uid(String, DiscoveryMethods),
    AddressWithUid(String, String, DiscoveryMethods),
}

impl Display for AddressOrUid {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        match self {
            AddressOrUid::AddressWithUid(addr, uid, _) => {
                write!(f, "Address: {}, UID: {}", addr, uid)
            }
            AddressOrUid::Address(host) => write!(f, "Address: {}", host),
            AddressOrUid::Uid(host, _) => write!(f, "UID: {}", host),
        }
    }
}

impl AddressOrUid {
    // Created by translating the config fields directly
    pub(crate) fn new(
        address: &Option<String>,
        uid: &Option<String>,
        method: &DiscoveryMethods,
    ) -> Result<Self, Error> {
        match (address, uid) {
            (None, None) => Err(anyhow!("Neither address or uid given")),
            (Some(host), Some(uid)) => Ok(AddressOrUid::AddressWithUid(
                host.clone(),
                uid.clone(),
                *method,
            )),
            (Some(host), None) => Ok(AddressOrUid::Address(host.clone())),
            (None, Some(host)) => Ok(AddressOrUid::Uid(host.clone(), *method)),
        }
    }

    // Convience method to get the BcCamera with the appropiate method
    // from a camera_config
    pub(crate) async fn connect_camera(
        &self,
        camera_config: &CameraConfig,
    ) -> Result<BcCamera, Error> {
        let (port, addrs) = {
            if let Some(addr_str) = camera_config.camera_addr.as_ref() {
                match addr_str.to_socket_addrs() {
                    Ok(addr_iter) => {
                        let mut port = None;
                        let mut ipaddrs = vec![];
                        for addr in addr_iter {
                            port = Some(addr.port());
                            ipaddrs.push(addr.ip());
                        }
                        Ok((port, ipaddrs))
                    }
                    Err(_) => match IpAddr::from_str(addr_str) {
                        Ok(ip) => Ok((None, vec![ip])),
                        Err(_) => Err(anyhow!("Could not parse address in config")),
                    },
                }
            } else {
                Ok((None, vec![]))
            }
        }?;

        let options = BcCameraOpt {
            name: camera_config.name.clone(),
            channel_id: camera_config.channel_id,
            addrs,
            port,
            uid: camera_config.camera_uid.clone(),
            protocol: ConnectionProtocol::TcpUdp,
            discovery: camera_config.discovery,
            credentials: Credentials {
                username: camera_config.username.clone(),
                password: camera_config.password.clone(),
            },
            debug: camera_config.debug,
            max_discovery_retries: camera_config.max_discovery_retries,
        };

        trace!("Camera Info: {:?}", options);

        Ok(BcCamera::new(&options).await?)
    }
}

pub(crate) async fn connect_and_login(camera_config: &CameraConfig) -> Result<BcCamera> {
    let camera_addr = AddressOrUid::new(
        &camera_config.camera_addr,
        &camera_config.camera_uid,
        &camera_config.discovery,
    )
    .unwrap();
    info!(
        "{}: Connecting to camera at {}",
        camera_config.name, camera_addr
    );

    let camera = camera_addr
        .connect_camera(camera_config)
        .await
        .with_context(|| {
            format!(
                "Failed to connect to camera {} at {} on channel {}",
                camera_config.name, camera_addr, camera_config.channel_id
            )
        })?;

    let max_encryption = match camera_config.max_encryption.to_lowercase().as_str() {
        "none" => MaxEncryption::None,
        "bcencrypt" => MaxEncryption::BcEncrypt,
        "aes" => MaxEncryption::Aes,
        _ => MaxEncryption::Aes,
    };
    info!("{}: Logging in", camera_config.name);
    timeout(camera.login_with_maxenc(max_encryption))
        .await
        .with_context(|| format!("Failed to login to {}", camera_config.name))??;

    info!("{}: Connected and logged in", camera_config.name);

    Ok(camera)
}

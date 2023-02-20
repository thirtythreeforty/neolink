//! Contains code that is not specific to any of the subcommands
//!
use log::*;

use super::config::{CameraConfig, Config};
use anyhow::{anyhow, Context, Error, Result};
use neolink_core::bc_protocol::{BcCamera, DiscoveryMethods};
use std::fmt::{Display, Error as FmtError, Formatter};

pub(crate) enum AddressOrUid {
    Address(String),
    Uid(String, DiscoveryMethods),
}

impl Display for AddressOrUid {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        match self {
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
        disc_method: &str,
    ) -> Result<Self, Error> {
        match (address, uid) {
            (None, None) => Err(anyhow!("Neither address or uid given")),
            (Some(_), Some(_)) => Err(anyhow!("Either address or uid should be given not both")),
            (Some(host), None) => Ok(AddressOrUid::Address(host.clone())),
            (None, Some(host)) => {
                let method = match disc_method.to_lowercase().as_str() {
                    "none" => DiscoveryMethods::None,
                    "local" => DiscoveryMethods::Local,
                    "remote" => DiscoveryMethods::Remote,
                    "relay" => DiscoveryMethods::Relay,
                    n => {
                        warn!("Unrecognised discovery method: {}. Using Local", n);
                        DiscoveryMethods::Local
                    }
                };
                Ok(AddressOrUid::Uid(host.clone(), method))
            }
        }
    }

    // Convience method to get the BcCamera with the appropiate method
    pub(crate) async fn connect_camera<T: Into<String>, U: Into<String>>(
        &self,
        channel_id: u8,
        username: T,
        passwd: Option<U>,
    ) -> Result<BcCamera, Error> {
        match self {
            AddressOrUid::Address(host) => {
                Ok(BcCamera::new_with_addr(host, channel_id, username, passwd).await?)
            }
            AddressOrUid::Uid(host, method) => {
                Ok(BcCamera::new_with_uid(host, channel_id, username, passwd, *method).await?)
            }
        }
    }
}

pub(crate) async fn find_and_connect(config: &Config, name: &str) -> Result<BcCamera> {
    let camera_config = find_camera_by_name(config, name)?;
    connect_and_login(camera_config).await
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
        .connect_camera(
            camera_config.channel_id,
            &camera_config.username,
            camera_config.password.as_ref(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to connect to camera {} at {} on channel {}",
                camera_config.name, camera_addr, camera_config.channel_id
            )
        })?;

    info!("{}: Logging in", camera_config.name);
    camera
        .login()
        .await
        .with_context(|| format!("Failed to login to {}", camera_config.name))?;

    info!("{}: Connected and logged in", camera_config.name);

    Ok(camera)
}

pub(crate) fn find_camera_by_name<'a>(config: &'a Config, name: &str) -> Result<&'a CameraConfig> {
    config
        .cameras
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| anyhow!("Camera {} not found in the config file", name))
}

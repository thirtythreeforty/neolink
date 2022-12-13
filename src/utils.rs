//! Contains code that is not specific to any of the subcommands
//!
use log::*;

use super::config::{CameraConfig, Config};
use anyhow::{anyhow, Context, Error, Result};
use neolink_core::bc_protocol::BcCamera;
use std::fmt::{Display, Error as FmtError, Formatter};

pub(crate) enum AddressOrUid {
    Address(String),
    Uid(String),
}

impl Display for AddressOrUid {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        match self {
            AddressOrUid::Address(host) => write!(f, "Address: {}", host),
            AddressOrUid::Uid(host) => write!(f, "UID: {}", host),
        }
    }
}

impl AddressOrUid {
    // Created by translating the config fields directly
    pub(crate) fn new(address: &Option<String>, uid: &Option<String>) -> Result<Self, Error> {
        match (address, uid) {
            (None, None) => Err(anyhow!("Neither address or uid given")),
            (Some(_), Some(_)) => Err(anyhow!("Either address or uid should be given not both")),
            (Some(host), None) => Ok(AddressOrUid::Address(host.clone())),
            (None, Some(host)) => Ok(AddressOrUid::Uid(host.clone())),
        }
    }

    // Convience method to get the BcCamera with the appropiate method
    pub(crate) fn connect_camera(&self, channel_id: u8) -> Result<BcCamera, Error> {
        match self {
            AddressOrUid::Address(host) => Ok(BcCamera::new_with_addr(host, channel_id)?),
            AddressOrUid::Uid(host) => Ok(BcCamera::new_with_uid(host, channel_id)?),
        }
    }
}

pub(crate) fn find_and_connect(config: &Config, name: &str) -> Result<BcCamera> {
    let camera_config = find_camera_by_name(config, name)?;
    connect_and_login(camera_config)
}

pub(crate) fn connect_and_login(camera_config: &CameraConfig) -> Result<BcCamera> {
    let camera_addr =
        AddressOrUid::new(&camera_config.camera_addr, &camera_config.camera_uid).unwrap();
    info!(
        "{}: Connecting to camera at {}",
        camera_config.name, camera_addr
    );

    let camera = camera_addr
        .connect_camera(camera_config.channel_id)
        .with_context(|| {
            format!(
                "Failed to connect to camera {} at {} on channel {}",
                camera_config.name, camera_addr, camera_config.channel_id
            )
        })?;

    info!("{}: Logging in", camera_config.name);
    camera
        .login(&camera_config.username, camera_config.password.as_deref())
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

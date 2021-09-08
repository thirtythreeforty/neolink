//! Contains code that is not specific to any of the subcommands
//!
use anyhow::{anyhow, Error};
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

//!
//! Shared data between all states
//!
//!
use anyhow::{Error, Result};
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;

use neolink_core::bc_protocol::{BcCamera, MaxEncryption, StreamKind as Stream};

use crate::config::PauseConfig;
use crate::rtsp::gst::NeoRtspServer;
use crate::utils::AddressOrUid;

#[allow(dead_code)]
pub(crate) struct Shared {
    pub(super) camera: Arc<BcCamera>,
    pub(super) name: String,
    pub(super) channel: u8,
    pub(super) addr: AddressOrUid,
    pub(super) username: String,
    pub(super) password: Option<String>,
    pub(super) streams: HashSet<Stream>,
    pub(super) rtsp: Arc<NeoRtspServer>,
    pub(super) permitted_users: HashSet<String>,
    pub(super) pause: PauseConfig,
    pub(super) max_encryption: MaxEncryption,
    pub(super) strict: bool,
}

impl Shared {
    pub(crate) fn get_tag_for_stream(&self, stream: &Stream) -> String {
        format!("{}::{:?}", self.name, stream)
    }
    pub(crate) fn get_paths_for_stream(&self, stream: &Stream) -> Vec<String> {
        vec![
            // Normal case
            format!("/{}/{:?}", self.name, stream),
            format!("/{}/{:?}Stream", self.name, stream),
            format!("/{}/{:?}stream", self.name, stream),
            // Lower case name
            format!("/{}/{:?}", self.name.to_lowercase(), stream),
            format!("/{}/{:?}Stream", self.name.to_lowercase(), stream),
            format!("/{}/{:?}stream", self.name.to_lowercase(), stream),
            // Lower case stream
            format!("/{}/{}", self.name, format!("{:?}", stream).to_lowercase()),
            format!(
                "/{}/{}Stream",
                self.name,
                format!("{:?}", stream).to_lowercase()
            ),
            format!(
                "/{}/{}stream",
                self.name,
                format!("{:?}", stream).to_lowercase()
            ),
            // Lower case both
            format!(
                "/{}/{}",
                self.name.to_lowercase(),
                format!("{:?}", stream).to_lowercase()
            ),
            format!(
                "/{}/{}Stream",
                self.name.to_lowercase(),
                format!("{:?}", stream).to_lowercase()
            ),
            format!(
                "/{}/{}stream",
                self.name.to_lowercase(),
                format!("{:?}", stream).to_lowercase()
            ),
        ]
    }
}

#[async_trait]
pub(crate) trait CameraState: Default {
    async fn setup(&mut self, shared: &Shared) -> Result<(), Error>;

    async fn tear_down(&mut self, shared: &Shared) -> Result<(), Error>;
}

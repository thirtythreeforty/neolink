//!
//! Shared data between all states
//!
//!
use anyhow::{Context, Result};
use log::*;
use std::collections::HashSet;
use std::sync::Arc;

use neolink_core::bc_protocol::StreamKind as Stream;

use crate::config::{CameraConfig, UserConfig};
use crate::rtsp::gst::NeoRtspServer;
use crate::utils::AddressOrUid;

#[allow(dead_code)]
pub(crate) struct Shared {
    pub(super) addr: AddressOrUid,
    pub(super) streams: HashSet<Stream>,
    pub(super) rtsp: Arc<NeoRtspServer>,
    pub(super) permitted_users: HashSet<String>,
    pub(super) config: CameraConfig,
}

impl Shared {
    pub(crate) async fn new(
        config: CameraConfig,
        users: &[UserConfig],
        rtsp: Arc<NeoRtspServer>,
    ) -> Result<Self> {
        let addr = AddressOrUid::new(&config.camera_addr, &config.camera_uid, &config.discovery)
            .with_context(|| {
                format!(
                    "Could not connect to camera {}. Please check UUID or IP:Port in the config",
                    config.name
                )
            })?;

        let mut streams: HashSet<Stream> = Default::default();
        if ["all", "both", "mainStream", "mainstream"]
            .iter()
            .any(|&e| e == config.stream)
        {
            streams.insert(Stream::Main);
        }
        if ["all", "both", "subStream", "substream"]
            .iter()
            .any(|&e| e == config.stream)
        {
            streams.insert(Stream::Sub);
        }
        if ["all", "externStream", "externstream"]
            .iter()
            .any(|&e| e == config.stream)
        {
            streams.insert(Stream::Extern);
        }

        let all_users_hash = || users.iter().map(|u| u.name.clone()).collect();
        let permitted_users: HashSet<String> = match &config.permitted_users {
            // If in the camera config there is the user "anyone", or if none is specified but users
            // are defined at all, then we add all users to the camera's allowed list.
            Some(p) if p.iter().any(|u| u == "anyone") => all_users_hash(),
            None if !users.is_empty() => all_users_hash(),

            // The user specified permitted_users
            Some(p) => p.iter().cloned().collect(),

            // The user didn't specify permitted_users, and there are none defined anyway
            None => ["anonymous".to_string()].iter().cloned().collect(),
        };

        let me = Shared {
            addr,
            streams,
            rtsp,
            permitted_users,
            config,
        };
        me.setup_streams().await?;
        Ok(me)
    }

    // Set up streams on the RTSP camera
    pub(crate) async fn setup_streams(&self) -> Result<()> {
        for stream in self.streams.iter() {
            let tag = self.get_tag_for_stream(stream);
            self.rtsp.create_stream(&tag, &self.config).await?;
            self.rtsp
                .add_permitted_roles(&tag, &self.permitted_users)
                .await?;
            let paths: Vec<String> = self.get_paths_for_stream(stream);
            debug!("Adding path {:?} for {}", paths, tag);
            self.rtsp.add_path(&tag, &paths).await?;
        }
        Ok(())
    }

    pub(crate) fn get_tags(&self) -> Vec<String> {
        self.streams
            .iter()
            .map(|k| self.get_tag_for_stream(k))
            .collect()
    }
    pub(crate) fn get_streams(&self) -> &HashSet<Stream> {
        &self.streams
    }
    pub(crate) fn get_config(&self) -> &CameraConfig {
        &self.config
    }
    pub(crate) fn get_tag_for_stream(&self, stream: &Stream) -> String {
        format!("{}::{:?}", self.config.name, stream)
    }
    pub(crate) fn get_paths_for_stream(&self, stream: &Stream) -> Vec<String> {
        let mut streams = vec![
            // Normal case
            format!("/{}/{:?}", self.config.name, stream),
            format!("/{}/{:?}Stream", self.config.name, stream),
            format!("/{}/{:?}stream", self.config.name, stream),
            // Lower case name
            format!("/{}/{:?}", self.config.name.to_lowercase(), stream),
            format!("/{}/{:?}Stream", self.config.name.to_lowercase(), stream),
            format!("/{}/{:?}stream", self.config.name.to_lowercase(), stream),
            // Lower case stream
            format!(
                "/{}/{}",
                self.config.name,
                format!("{:?}", stream).to_lowercase()
            ),
            format!(
                "/{}/{}Stream",
                self.config.name,
                format!("{:?}", stream).to_lowercase()
            ),
            format!(
                "/{}/{}stream",
                self.config.name,
                format!("{:?}", stream).to_lowercase()
            ),
            // Lower case both
            format!(
                "/{}/{}",
                self.config.name.to_lowercase(),
                format!("{:?}", stream).to_lowercase()
            ),
            format!(
                "/{}/{}Stream",
                self.config.name.to_lowercase(),
                format!("{:?}", stream).to_lowercase()
            ),
            format!(
                "/{}/{}stream",
                self.config.name.to_lowercase(),
                format!("{:?}", stream).to_lowercase()
            ),
        ];

        if (self.streams.contains(&Stream::Main) && matches!(stream, Stream::Main))
            || (!self.streams.contains(&Stream::Main) && matches!(stream, Stream::Sub))
            || (!self.streams.contains(&Stream::Main)
                && !self.streams.contains(&Stream::Sub)
                && matches!(stream, Stream::Extern))
        {
            // If main stream add that
            streams.push(format!("/{}", self.config.name));
            streams.push(format!("/{}", self.config.name.to_lowercase()));
        }
        streams
    }
}

//!
//! Shared data between all states
//!
//!
use anyhow::{Error, Result};
use std::collections::HashSet;
use std::sync::Arc;

use neolink_core::bc_protocol::{BcCamera, Stream};

use crate::config::PauseConfig;
use crate::rtsp::gst::RtspServer;
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
    pub(super) rtsp: Arc<RtspServer>,
    pub(super) permitted_users: HashSet<String>,
    pub(super) pause: PauseConfig,
}

impl Shared {
    pub(super) fn get_all_paths(&self) -> Vec<String> {
        self.streams
            .iter()
            .flat_map(|s| self.get_paths(s))
            .collect()
    }

    pub(super) fn get_paths(&self, stream: &Stream) -> Vec<String> {
        let mut streams = match stream {
            Stream::Main => vec![
                format!("/{}", &self.name),
                format!("/{}/mainStream", &self.name),
            ],
            Stream::Sub => vec![format!("/{}/subStream", &self.name)],
            Stream::Extern => {
                vec![format!("/{}/externStream", &self.name)]
            }
        };
        // Later VLC clients seem to only support lower case streams
        let mut lowercase_streams: Vec<String> = streams.iter().map(|i| i.to_lowercase()).collect();
        streams.append(&mut lowercase_streams);
        streams
    }
}

pub(crate) trait CameraState: Default {
    fn setup(&mut self, shared: &Shared) -> Result<(), Error>;

    fn tear_down(&mut self, shared: &Shared) -> Result<(), Error>;
}

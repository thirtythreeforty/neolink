//! Generic state
//!
//! This is the base state
use super::shared::Shared;
use crate::config::CameraConfig;
use std::sync::Arc;

pub(crate) struct Camera<T> {
    pub(crate) state: T,
    pub(crate) shared: Arc<Shared>,
}

impl<T> Camera<T> {
    pub(crate) fn get_name(&self) -> String {
        self.shared.config.name.clone()
    }

    #[allow(dead_code)]
    pub(crate) fn get_config(&self) -> &CameraConfig {
        &self.shared.config
    }

    pub(crate) fn get_rtsp(&self) -> Arc<super::super::gst::NeoRtspServer> {
        self.shared.rtsp.clone()
    }
}

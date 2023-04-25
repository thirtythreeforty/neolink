//! Data shared between the various
//! components that manage a media stream
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use tokio::sync::RwLock;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum VidFormats {
    Unknown,
    H264,
    H265,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum AudFormats {
    Unknown,
    Aac,
    Adpcm(u16),
}

pub(super) struct NeoMediaShared {
    pub(super) vid_format: RwLock<VidFormats>,
    pub(super) aud_format: RwLock<AudFormats>,
    pub(super) number_of_clients: AtomicUsize,
    pub(super) buffer_ready: AtomicBool,
}

impl Default for NeoMediaShared {
    fn default() -> Self {
        Self {
            vid_format: RwLock::new(VidFormats::Unknown),
            aud_format: RwLock::new(AudFormats::Unknown),
            number_of_clients: AtomicUsize::new(0),
            buffer_ready: AtomicBool::new(false),
        }
    }
}

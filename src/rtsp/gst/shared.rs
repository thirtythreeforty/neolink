//! Data shared between the various
//! components that manage a media stream
use gstreamer_app::AppSrc;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use std::sync::atomic::{AtomicU64, AtomicUsize, AtomicBool};
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
    pub(super) microseconds: AtomicU64,
    pub(super) number_of_clients: AtomicUsize,
    pub(super) buffer_ready: AtomicBool,
}

impl Default for NeoMediaShared {
    fn default() -> Self {
        Self {
            vid_format: RwLock::new(VidFormats::Unknown),
            aud_format: RwLock::new(AudFormats::Unknown),
            microseconds: AtomicU64::new(0),
            number_of_clients: AtomicUsize::new(0),
            buffer_ready: AtomicBool::new(false),
        }
    }
}

#[derive(Default, Debug)]
pub(super) struct ClientPipelineData {
    pub(super) vidsrc: Option<AppSrc>,
    pub(super) audsrc: Option<AppSrc>,
    pub(super) start_time: u64,
    pub(super) inited: bool,
}

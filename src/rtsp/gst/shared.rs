//! Data shared between the various
//! components that manage a media stream
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    pub(super) buffer_size: AtomicUsize,
    pub(super) use_smoothing: AtomicBool,
    pub(super) splash: AtomicBool,
}

impl Default for NeoMediaShared {
    fn default() -> Self {
        Self {
            vid_format: RwLock::new(VidFormats::Unknown),
            aud_format: RwLock::new(AudFormats::Unknown),
            number_of_clients: AtomicUsize::new(0),
            buffer_ready: AtomicBool::new(false),
            buffer_size: AtomicUsize::new(100),
            use_smoothing: AtomicBool::new(false),
            splash: AtomicBool::new(true),
        }
    }
}

impl NeoMediaShared {
    pub(super) fn get_buffer_size(&self) -> usize {
        self.buffer_size.load(Ordering::Relaxed)
    }

    pub(super) fn set_buffer_size(&self, new_size: usize) {
        self.buffer_size.store(new_size, Ordering::Relaxed)
    }

    pub(super) fn get_use_smoothing(&self) -> bool {
        self.use_smoothing.load(Ordering::Relaxed)
    }

    pub(super) fn set_use_smoothing(&self, new_value: bool) {
        self.use_smoothing.store(new_value, Ordering::Relaxed)
    }

    pub(super) fn get_use_splash(&self) -> bool {
        self.splash.load(Ordering::Relaxed)
    }

    pub(super) fn set_use_splash(&self, new_value: bool) {
        self.splash.store(new_value, Ordering::Relaxed)
    }
}

//! Data shared between the various
//! components that manage a media stream
use atomic_enum::atomic_enum;

pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use std::sync::atomic::AtomicU64;

#[atomic_enum]
#[derive(PartialEq)]
pub(super) enum VidFormats {
    Unknown = 0,
    H264,
    H265,
}

#[atomic_enum]
#[derive(PartialEq)]
pub(super) enum AudFormats {
    Unknown = 0,
    Aac,
    Adpcm,
}

pub(super) struct NeoMediaShared {
    pub(super) vid_format: AtomicVidFormats,
    pub(super) aud_format: AtomicAudFormats,
    pub(super) microseconds: AtomicU64,
}

impl Default for NeoMediaShared {
    fn default() -> Self {
        Self {
            vid_format: AtomicVidFormats::new(VidFormats::Unknown),
            aud_format: AtomicAudFormats::new(AudFormats::Unknown),
            microseconds: AtomicU64::new(0),
        }
    }
}

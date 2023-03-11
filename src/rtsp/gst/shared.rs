//! Data shared between the various
//! components that manage a media stream
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use std::convert::{From, Into};
use std::sync::atomic::AtomicU64;

#[derive(PartialEq, Eq, Debug)]
pub(super) enum VidFormats {
    Unknown = 0,
    H264,
    H265,
}

impl From<VidFormats> for u64 {
    fn from(value: VidFormats) -> u64 {
        match value {
            VidFormats::Unknown => 0,
            VidFormats::H264 => 1,
            VidFormats::H265 => 2,
        }
    }
}

impl From<u64> for VidFormats {
    fn from(value: u64) -> Self {
        match value {
            0 => VidFormats::Unknown,
            1 => VidFormats::H264,
            2 => VidFormats::H265,
            _ => unreachable!(),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub(super) enum AudFormats {
    Unknown = 0,
    Aac,
    Adpcm,
}

impl From<AudFormats> for u64 {
    fn from(value: AudFormats) -> u64 {
        match value {
            AudFormats::Unknown => 0,
            AudFormats::Aac => 1,
            AudFormats::Adpcm => 2,
        }
    }
}

impl From<u64> for AudFormats {
    fn from(value: u64) -> Self {
        match value {
            0 => AudFormats::Unknown,
            1 => AudFormats::Aac,
            2 => AudFormats::Adpcm,
            _ => unreachable!(),
        }
    }
}

pub(super) struct NeoMediaShared {
    pub(super) vid_format: AtomicU64,
    pub(super) aud_format: AtomicU64,
    pub(super) microseconds: AtomicU64,
}

impl Default for NeoMediaShared {
    fn default() -> Self {
        Self {
            vid_format: AtomicU64::new(VidFormats::Unknown.into()),
            aud_format: AtomicU64::new(AudFormats::Unknown.into()),
            microseconds: AtomicU64::new(0),
        }
    }
}

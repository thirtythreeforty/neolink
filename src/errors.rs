use thiserror::Error;

#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error("RTSP Error")]
    Rtsp(#[from] super::rtsp::Error),
    #[error("Status LED Error")]
    StatusLight(#[from] super::statusled::Error),
    #[error("Reboot Error")]
    Reboot(#[from] super::reboot::Error),
    #[error("Talk Error")]
    Talk(#[from] super::talk::Error),
    #[error("Talk Error")]
    Mqtt(#[from] super::mqtt::Error),
}

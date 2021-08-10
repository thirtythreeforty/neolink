use err_derive::Error;

#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(display = "RTSP Error")]
    Rtsp(#[error(source)] super::rtsp::Error),
    #[error(display = "Status LED Error")]
    StatusLight(#[error(source)] super::statusled::Error),
    #[error(display = "Reboot Error")]
    Reboot(#[error(source)] super::reboot::Error),
    #[error(display = "Talk Error")]
    Talk(#[error(source)] super::talk::Error),
    #[error(display = "Talk Error")]
    Mqtt(#[error(source)] super::mqtt::Error),
}

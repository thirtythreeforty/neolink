use err_derive::Error;

#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(display = "RTSP Error")]
    RtspError(#[error(source)] neolink_rtsp::errors::Error),
}

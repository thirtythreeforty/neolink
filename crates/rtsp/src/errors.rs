use err_derive::Error;

#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(display = "Configuration parsing error")]
    ConfigError(#[error(source)] toml::de::Error),
    #[error(display = "Communication error")]
    ProtocolError(#[error(source)] neolink_core::Error),
    #[error(display = "I/O error")]
    IoError(#[error(source)] std::io::Error),
    #[error(display = "Validation error")]
    ValidationError(#[error(source)] validator::ValidationErrors),
    #[error(display = "ADPCM Decoding Error")]
    AdpcmDecodingError(&'static str),
}

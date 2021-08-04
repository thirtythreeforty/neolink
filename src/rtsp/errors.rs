use err_derive::Error;

/// The main error for the rtsp subcommand
#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    /// Raised when the config file fails to deserlize
    #[error(display = "Configuration parsing error")]
    Config(#[error(source)] toml::de::Error),
    /// Raised when `neolink_core` raises an error
    #[error(display = "Communication error")]
    Protocol(#[error(source)] neolink_core::Error),
    /// Raised when there is an IO error such as unable to find
    /// config file
    #[error(display = "I/O error")]
    Io(#[error(source)] std::io::Error),
    /// Raised when the config file fails validataion
    #[error(display = "Validation error")]
    Validation(#[error(source)] validator::ValidationErrors),
    // Raised when there is an ADPCM decoding error
    // #[error(display = "ADPCM Decoding Error")]
    // AdpcmDecoding(&'static str),
}

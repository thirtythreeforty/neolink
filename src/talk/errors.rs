use err_derive::Error;
use gstreamer::glib;

/// The main error for the status-light subcommand
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
    /// Raised when gstreamer fails to init
    #[error(display = "Gstreamer init failed")]
    GstreamerInit(#[error(source)] glib::Error),
    /// Raised when a needed part of the gsteamer pipeline is not found
    ///
    /// Usually this means missing pluings
    #[error(display = "Gstreamer element not supported. Check your gstreamer plugins")]
    GstreamerElement(gstreamer::Element),
    /// Raised where there is an issue starting a audio playback in gstraemer
    #[error(display = "Gstreamer Unable to play the file")]
    GstreamerPlayback(#[error(source)] gstreamer::StateChangeError),
    /// Generic gstreamer error
    #[error(display = "Gstreamer raised an error")]
    Gstreamer {
        error: String,
        debug: Option<String>,
    },

    /// Raised when talk back is unsupported
    #[error(display = "This camera does not support talkback")]
    TalkUnsupported,
}

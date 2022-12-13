use super::bc::model::Bc;
use err_derive::Error;

/// This is the primary error type of the library
#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    /// Error raised during deserlization
    #[error(display = "Deserialization error")]
    Deserialization(#[error(source)] super::bc::de::Error),

    /// Error raised during serlization
    #[error(display = "Serialization error")]
    Serialization(#[error(source)] super::bc::ser::Error),

    /// Error raised during deserlization
    #[error(display = "Media Deserialization error")]
    MediaDeserialization(#[error(source)] super::bcmedia::de::Error),

    /// Error raised during serlization
    #[error(display = "Media Serialization error")]
    MediaSerialization(#[error(source)] super::bcmedia::ser::Error),

    /// A connection error such as Simultaneous subscription
    #[error(display = "Connection error")]
    ConnectionError(#[error(source)] super::connection::Error),

    /// Raised when a Bc reply was not understood
    #[error(display = "Communication error")]
    UnintelligibleReply {
        /// The Bc packet that was not understood
        reply: Box<Bc>,
        /// The message attached to the error
        why: &'static str,
    },

    /// Raised when the camera responds with a status code over than OK
    #[error(display = "Camera responded with Service Unavaliable")]
    CameraServiceUnavaliable,

    /// Raised when a connection is dropped.
    #[error(display = "Dropped connection")]
    DroppedConnection(#[error(source)] crossbeam_channel::RecvError),

    /// Raised when a connection is dropped during a TryRecv event
    #[error(display = "Dropped connection")]
    DroppedConnectionTry(#[error(source)] crossbeam_channel::TryRecvError),

    /// Raised when the RX_TIMEOUT is reach
    #[error(display = "Timeout")]
    Timeout,

    /// Raised when connection is dropped because the timeout is reach
    #[error(display = "Dropped connection")]
    TimeoutDisconnected,

    /// Raised when failed to login to the camera
    #[error(display = "Credential error")]
    AuthFailed,

    /// Raised when the given camera url could not be resolved
    #[error(display = "Failed to translate camera address")]
    AddrResolutionError,

    /// Raised non adpcm data is sent to the talk command
    #[error(display = "Talk data is not ADPCM")]
    UnknownTalkEncoding,

    /// A generic catch all error
    #[error(display = "Other error")]
    Other(&'static str),

    /// A generic catch all error
    #[error(display = "Other error")]
    OtherString(String),
}

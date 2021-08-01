use super::bc::model::Bc;
use err_derive::Error;

/// This is the primary error type of the library
#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    /// Error raised when IO fails such as when the connection is lost
    #[error(display = "Communication error")]
    Communication(#[error(source)] std::io::Error),

    /// Errors raised during deserlization
    #[error(display = "Deserialization error")]
    Deserialization(#[error(source)] super::bc::de::Error),

    /// Errors raised during serlization
    #[error(display = "Serialization error")]
    Serialization(#[error(source)] super::bc::ser::Error),

    /// A connection error such as Simultaneous subscription
    #[error(display = "Connection error")]
    ConnectionError(#[error(source)] super::connection::Error),

    /// Raised when a Bc reply was not understood
    #[error(display = "Communication error")]
    UnintelligibleReply {
        /// The Bc packet that was not understood
        reply: Bc,
        /// The message attached to the error
        why: &'static str,
    },

    /// Raised when a connection is dropped. This can be for many reasons
    /// and is usually not helpful
    #[error(display = "Dropped connection")]
    DroppedConnection(#[error(source)] std::sync::mpsc::RecvError),

    /// Raised when the RX_TIMEOUT is reach
    #[error(display = "Timeout")]
    Timeout,

    /// Raised when connection is dropped because the RX_TIMEOUT is reach
    #[error(display = "Dropped connection")]
    TimeoutDisconnected,

    /// Raised when failed to login to the camera
    #[error(display = "Credential error")]
    AuthFailed,

    /// Raised when the given camera url could not be resolved
    #[error(display = "Failed to translate camera address")]
    AddrResolutionError,

    /// A generic catch all error
    #[error(display = "Other error")]
    Other(&'static str),

    /// A generic catch all error
    #[error(display = "Other error")]
    OtherString(String),
}

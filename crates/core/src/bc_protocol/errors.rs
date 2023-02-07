use super::bc::model::Bc;
use crate::NomErrorType;
use err_derive::Error;

/// This is the primary error type of the library
#[derive(Debug, Error, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    /// Underlying IO errors
    #[error(display = "IO Error: {:?}", _0)]
    Io(#[error(source)] std::sync::Arc<std::io::Error>),

    /// Raised when a Bc reply was not understood
    #[error(display = "Communication error")]
    UnintelligibleReply {
        /// The Bc packet that was not understood
        reply: std::sync::Arc<Box<Bc>>,
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

    /// Raised when a connection is dropped during a tokio mpsc TryRecv event
    #[error(display = "Dropped connection")]
    TokioDroppedConnectionTry(#[error(source)] tokio::sync::mpsc::error::TryRecvError),

    /// Raised when a connection is dropped during a TryRecv event
    #[error(display = "Dropped connection")]
    TokioBroadcastDroppedConnectionTry(
        #[error(source)] tokio::sync::broadcast::error::TryRecvError,
    ),

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

    /// Raised when dicovery times out waiting for a reply
    #[error(display = "Timed out while waiting for camera reply")]
    DiscoveryTimeout,

    /// Raised during a (de)seralisation error
    #[error(display = "Cookie GenError")]
    GenError(#[error(source)] std::sync::Arc<cookie_factory::GenError>),

    /// Raised when a connection is subscrbed to more than once
    #[error(display = "Simultaneous subscription, {}", _0)]
    SimultaneousSubscription {
        /// The message number that was subscribed to
        msg_num: u16,
    },

    /// Raised when a new encyrption byte is observed
    #[error(display = "Unknown encryption: {:x?}", _0)]
    UnknownEncryption(usize),

    /// Raised when the camera cannot be found
    #[error(display = "Camera Not Findable")]
    ConnectionUnavaliable,

    /// Raised when the subscription id dropped too soon
    #[error(display = "Dropped Subscriber")]
    DroppedSubscriber,

    /// Raised when a unknown connection ID attempts to connect with us over UDP
    #[error(display = "Connection with unknown connectionID: {:?}", _0)]
    UnknownConnectionId(i32),

    /// Raised when a unknown SocketAddr attempts to connect with us over UDP
    #[error(display = "Connection from unknown source: {:?}", _0)]
    UnknownSource(std::net::SocketAddr),

    /// Raised when the IP/Hostname cannot be understood
    #[error(display = "Could not parse as IP")]
    AddrParseError(#[error(source)] std::net::AddrParseError),

    /// Raised when the stream is not enough to complete a message
    #[error(display = "Nom Parsing incomplete: {}", _0)]
    NomIncomplete(usize),

    /// Raised when a stream cannot be decoded
    #[error(display = "Nom Parsing error: {}", _0)]
    NomError(String),

    /// A generic catch all error
    #[error(display = "Other error: {}", _0)]
    Other(&'static str),

    /// A generic catch all error
    #[error(display = "Other error: {}", _0)]
    OtherString(String),
}

impl From<std::io::Error> for Error {
    fn from(k: std::io::Error) -> Self {
        Error::Io(std::sync::Arc::new(k))
    }
}

impl From<cookie_factory::GenError> for Error {
    fn from(k: cookie_factory::GenError) -> Self {
        Error::GenError(std::sync::Arc::new(k))
    }
}

impl<'a> From<nom::Err<NomErrorType<'a>>> for Error {
    fn from(k: nom::Err<NomErrorType<'a>>) -> Self {
        match k {
            nom::Err::Error(e) => Error::NomError(format!("Nom Error: {:?}", e)),
            nom::Err::Failure(e) => Error::NomError(format!("Nom Error: {:?}", e)),
            nom::Err::Incomplete(nom::Needed::Size(amount)) => Error::NomIncomplete(amount.get()),
            nom::Err::Incomplete(nom::Needed::Unknown) => Error::NomIncomplete(1),
        }
    }
}

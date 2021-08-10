use err_derive::Error;
use std::convert::From;
use std::sync::{MutexGuard, PoisonError};

/// The main error for the reboot subcommand
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
    /// Raised when crossbeam fails to recv
    #[error(display = "Recv data error")]
    CrossbeamRecv(#[error(source)] crossbeam_channel::RecvError),
    #[error(display = "Send data error")]
    CrossbeamSend,
    #[error(display = "Unable to lock mutex")]
    Lock,
}

impl<M> From<crossbeam_channel::SendError<M>> for Error {
    fn from(_: crossbeam_channel::SendError<M>) -> Self {
        Error::CrossbeamSend
    }
}

impl<'a, M> From<PoisonError<MutexGuard<'a, M>>> for Error {
    fn from(_: PoisonError<MutexGuard<'a, M>>) -> Self {
        Error::Lock
    }
}

//! This module provides an "RtspServer" abstraction that allows consumers of its API to feed it
//! data using an ordinary std::io::Write interface.

mod factory;
mod sender;
mod server;
mod shared;

use factory::*;

pub(crate) use self::server::NeoRtspServer;
pub(crate) use gstreamer_rtsp_server::gio::TlsAuthenticationMode;

type AnyResult<T> = std::result::Result<T, anyhow::Error>;

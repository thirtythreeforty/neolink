//! This module provides an "RtspServer" abstraction that allows consumers of its API to feed it
//! data using an ordinary std::io::Write interface.

mod factory;
mod server;
mod shared;

pub(crate) use factory::*;

pub(crate) use self::server::NeoRtspServer;

type AnyResult<T> = std::result::Result<T, anyhow::Error>;

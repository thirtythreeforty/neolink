//! Handles the camera in it's different states
//!

mod camera;
mod connected;
mod disconnected;
mod loggedin;
mod shared;
mod streaming;

pub(crate) use self::{
    camera::*, disconnected::*, loggedin::*, shared::*,
};

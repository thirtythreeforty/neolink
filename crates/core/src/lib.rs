#![warn(missing_docs)]
//! # Neolink-Core
//!
//! Neolink-Core is a rust library for interacting with reolink and family cameras.
//!
//! Most high level camera controls are in the [`bc_protocol`] module
//!
//! A camera can be initialised with
//!
//! ```no_run
//! let channel_id = 0; // Usually zero but can be non zero if uses a reolink NVR
//! let camera = BcCamera::new_with_addr("camera_ip_address", channel_id);
//! ```
//!
//! After that login can be conducted with
//!
//! ```no_run
//! # let channel_id = 0;
//! # let camera = BcCamera::new_with_addr("camera_ip_address", channel_id);
//! camera.login("username", "password");
//! ```
//! For further commands see the [`bc_protocol::BcCamera`] struct.
//!

/// Contains low level BC structures and formats
pub mod bc;
/// Contains high level interfaces for the camera
pub mod bc_protocol;

#[derive(Debug)]
/// Certain method just as `start_video` will block forever or return an error
/// In such a case the return type is `Result<Never, Error>`
pub enum Never {}

/// This is the top level error structure of the library
///
/// Most commands will either return their `Ok(result)` or this `Err(Error)`
pub use bc_protocol::Error;

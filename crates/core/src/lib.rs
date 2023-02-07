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
//! use neolink_core::bc_protocol::BcCamera;
//! let channel_id = 0; // Usually zero but can be non zero if uses a reolink NVR
//! let mut camera = BcCamera::new_with_addr("camera_ip_address", channel_id).unwrap();
//! ```
//!
//! After that login can be conducted with
//!
//! ```no_run
//! # use neolink_core::bc_protocol::BcCamera;
//! # let channel_id = 0;
//! # let mut camera = BcCamera::new_with_addr("camera_ip_address", channel_id).unwrap();
//! camera.login("username", Some("password"));
//! ```
//! For further commands see the [`bc_protocol::BcCamera`] struct.
//!

/// Contains low level BC structures and formats
pub mod bc;
/// Contains high level interfaces for the camera
pub mod bc_protocol;
/// Contains low level structures and formats for the media substream
pub mod bcmedia;
///  Contains low level structures and formats for the udpstream
pub mod bcudp;

/// This is the top level error structure of the library
///
/// Most commands will either return their `Ok(result)` or this `Err(Error)`
pub use bc_protocol::Error;

pub(crate) use bc_protocol::Result;

pub(crate) type NomErrorType<'a> = nom::error::VerboseError<&'a [u8]>;

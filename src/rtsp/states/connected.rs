// Data for the basic connected state
//
// This state has formed the TCP/UDP tunnel
// but has not logged in
use super::{CameraState, Shared};
use anyhow::{Error, Result};

#[derive(Default)]
pub(crate) struct Connected {}

impl CameraState for Connected {
    fn setup(&mut self, _shared: &Shared) -> Result<(), Error> {
        Ok(())
    }
    fn tear_down(&mut self, _shared: &Shared) -> Result<(), Error> {
        Ok(())
    }
}

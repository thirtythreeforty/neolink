// Data for the basic connected state
//
// This state has formed the TCP/UDP tunnel
// but has not logged in
use super::{CameraState, Shared};
use anyhow::{Error, Result};
use async_trait::async_trait;

#[derive(Default)]
pub(crate) struct Connected {}

#[async_trait]
impl CameraState for Connected {
    async fn setup(&mut self, _shared: &Shared) -> Result<(), Error> {
        Ok(())
    }
    async fn tear_down(&mut self, _shared: &Shared) -> Result<(), Error> {
        Ok(())
    }
}

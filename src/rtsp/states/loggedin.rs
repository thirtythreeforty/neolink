// Data for the logged in state
//
// This state is logged in but is not
// streaming data
//
// It can be used to alter settings etc

use super::{CameraState, Shared};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Default)]
pub(crate) struct LoggedIn {}

#[async_trait]
impl CameraState for LoggedIn {
    async fn setup(&mut self, shared: &Shared) -> Result<(), anyhow::Error> {
        shared.camera.login().await?;
        Ok(())
    }

    async fn tear_down(&mut self, shared: &Shared) -> Result<(), anyhow::Error> {
        shared.camera.logout().await?;
        Ok(())
    }
}

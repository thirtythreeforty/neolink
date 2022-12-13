// Data for the logged in state
//
// This state is logged in but is not
// streaming data
//
// It can be used to alter settings etc

use super::{CameraState, Shared};
use anyhow::Result;

#[derive(Default)]
pub(crate) struct LoggedIn {}

impl CameraState for LoggedIn {
    fn setup(&mut self, shared: &Shared) -> Result<(), anyhow::Error> {
        shared
            .camera
            .login(&shared.username, shared.password.as_deref())?;
        Ok(())
    }

    fn tear_down(&mut self, shared: &Shared) -> Result<(), anyhow::Error> {
        shared.camera.logout()?;
        Ok(())
    }
}

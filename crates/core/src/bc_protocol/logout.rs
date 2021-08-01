use super::{BcCamera, Result};

impl BcCamera {
    /// Logout from the camera
    pub fn logout(&mut self) -> Result<()> {
        if self.logged_in {
            // TODO: Send message ID 2
        }
        self.logged_in = false;
        Ok(())
    }
}

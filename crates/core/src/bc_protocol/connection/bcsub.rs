use super::{BcConnection, Result};
use crate::bc::model::*;
use std::sync::mpsc::Receiver;

pub struct BcSubscription<'a> {
    pub rx: Receiver<Bc>,
    msg_id: u32,
    conn: &'a BcConnection,
}

impl<'a> BcSubscription<'a> {
    pub fn new(rx: Receiver<Bc>, msg_id: u32, conn: &'a BcConnection) -> BcSubscription<'a> {
        BcSubscription { rx, msg_id, conn }
    }

    pub fn send(&self, bc: Bc) -> Result<()> {
        assert!(bc.meta.msg_id == self.msg_id);
        self.conn.send(bc)?;
        Ok(())
    }
}

/// Makes it difficult to avoid unsubscribing when you're finished
impl<'a> Drop for BcSubscription<'a> {
    fn drop(&mut self) {
        // It's fine if we can't unsubscribe as that means we already have
        let _ = self.conn.unsubscribe(self.msg_id);
    }
}

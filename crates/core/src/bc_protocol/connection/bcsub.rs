use super::{BcConnection, Result};
use crate::bc::model::*;
use crossbeam_channel::Receiver;

pub struct BcSubscription<'a> {
    pub rx: Receiver<Bc>,
    msg_num: u16,
    conn: &'a BcConnection,
}

impl<'a> BcSubscription<'a> {
    pub fn new(rx: Receiver<Bc>, msg_num: u16, conn: &'a BcConnection) -> BcSubscription<'a> {
        BcSubscription { rx, msg_num, conn }
    }

    pub fn send(&self, bc: Bc) -> Result<()> {
        assert!(bc.meta.msg_num == self.msg_num);
        self.conn.send(bc)?;
        Ok(())
    }
}

/// Makes it difficult to avoid unsubscribing when you're finished
impl<'a> Drop for BcSubscription<'a> {
    fn drop(&mut self) {
        // It's fine if we can't unsubscribe as that means we already have
        let _ = self.conn.unsubscribe(self.msg_num);
    }
}

use super::abort::AbortHandle;
use crossbeam_channel::{unbounded, Receiver, Sender};
use neolink_core::bc_protocol::{MotionOutput, MotionOutputError, MotionStatus};

pub(crate) struct MotionStream {
    tx: Sender<MotionStatus>,
    live: AbortHandle,
}

impl MotionStream {
    pub(crate) fn new() -> (Self, Receiver<MotionStatus>) {
        let (tx, rx) = unbounded();
        (
            Self {
                tx,
                live: AbortHandle::new(),
            },
            rx,
        )
    }

    pub(crate) fn get_abort_handle(&self) -> AbortHandle {
        self.live.clone()
    }
}

impl MotionOutput for MotionStream {
    fn motion_recv(&mut self, motion_status: MotionStatus) -> MotionOutputError {
        let _ = self.tx.send(motion_status);
        Ok(self.live.is_live())
    }
}

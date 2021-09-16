use super::abort::AbortHandle;
use crossbeam_channel::{unbounded, Receiver, Sender};
use neolink_core::bc_protocol::{MotionOutput, MotionOutputError, MotionStatus};
use std::time::Duration;

pub(crate) struct MotionStream {
    tx: Sender<MotionStatus>,
    live: AbortHandle,
    timeout: Duration,
    cooloff: Option<AbortHandle>,
}

impl MotionStream {
    pub(crate) fn new(timeout_seconds: f64) -> (Self, Receiver<MotionStatus>) {
        let (tx, rx) = unbounded();
        (
            Self {
                tx,
                live: AbortHandle::new(),
                timeout: Duration::from_secs_f64(timeout_seconds),
                cooloff: None,
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
        match motion_status {
            MotionStatus::Start => {
                if let Some(handle) = &self.cooloff {
                    handle.abort();
                    self.cooloff = None;
                }
                let _ = self.tx.send(motion_status);
            }
            MotionStatus::Stop => {
                // Abort any current one
                if let Some(handle) = &self.cooloff {
                    handle.abort();
                    self.cooloff = None;
                }
                let aborter = AbortHandle::new();
                let thread_duration = self.timeout;
                let thread_tx = self.tx.clone();
                let thread_message = MotionStatus::Stop;
                std::thread::spawn(move || {
                    std::thread::sleep(thread_duration);
                    if aborter.is_live() {
                        let _ = thread_tx.send(thread_message);
                    }
                });
            }
            _ => {
                let _ = self.tx.send(motion_status);
            }
        }

        Ok(self.live.is_live())
    }
}

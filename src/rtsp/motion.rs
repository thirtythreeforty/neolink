use super::abort::AbortHandle;
use super::state::States;
use neolink_core::bc_protocol::{MotionOutput, MotionOutputError, MotionStatus};
use std::time::Duration;

pub(crate) struct MotionStream {
    state: States,
    timeout: Duration,
    cooloff: Option<AbortHandle>,
}

impl MotionStream {
    pub(crate) fn new(timeout_seconds: f64, state: States) -> Self {
        Self {
            state,
            timeout: Duration::from_secs_f64(timeout_seconds),
            cooloff: None,
        }
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
                self.state.set_motion_detected(true);
            }
            MotionStatus::Stop => {
                // Abort any current one
                if let Some(handle) = &self.cooloff {
                    handle.abort();
                    self.cooloff = None;
                }
                let aborter = AbortHandle::new();
                let thread_duration = self.timeout;
                let thread_state = self.state.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(thread_duration);
                    if aborter.is_live() {
                        thread_state.set_motion_detected(false);
                    }
                });
            }
            _ => {}
        }

        Ok(self.state.is_live())
    }
}

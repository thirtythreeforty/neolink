use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone)]
pub struct AbortHandle {
    aborted: Arc<AtomicBool>,
}

impl AbortHandle {
    pub fn new() -> Self {
        Self {
            aborted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }
}

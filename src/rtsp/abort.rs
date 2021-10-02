use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone)]
pub(crate) struct AbortHandle {
    live: Arc<AtomicBool>,
}

impl AbortHandle {
    pub(crate) fn new() -> Self {
        Self {
            live: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) fn abort(&self) {
        self.live.store(false, Ordering::Relaxed);
    }

    pub(crate) fn is_live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }
}

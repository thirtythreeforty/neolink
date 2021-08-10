use std::collections::HashSet;
use std::sync::{Arc, Mutex};

// Handles app wide things like aborting all connections
pub(crate) struct App {
    aborters: Arc<Mutex<HashSet<String>>>,
}

impl App {
    pub(crate) fn new() -> Self {
        Self {
            aborters: Default::default(),
        }
    }

    // Abort at a level
    // e.g. about camera_event_loop
    pub(crate) fn abort(&self, level: &str) {
        let mut aborters = self.aborters.lock().unwrap();
        aborters.insert(level.to_string());
    }

    // Checks if any level has aborted
    // e.g. app:camera_a::camera_event_loop
    // will check if app, camera_a or camera_event_loop has been aborted
    pub(crate) fn running(&self, levels: &str) -> bool {
        let aborters = self.aborters.lock().unwrap();
        for level in levels.split(':') {
            if aborters.contains(&level.to_string()) {
                return false;
            }
        }
        true
    }
}

//! Used to handle the stream states
//!
//! When should stream is true then neolink
//! pulls the video from the camera.
//!
//! Otherwise it should use a fallback image
//!
use super::CameraConfig;
use log::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

enum MotionState {
    Motion,
    Still,
    Ignore,
}

enum RtspState {
    Connected,
    Disconnected,
    Ignore,
}

#[derive(Clone)]
pub(crate) struct States {
    motion: Arc<Mutex<MotionState>>,
    client: Arc<Mutex<RtspState>>,
    live: Arc<AtomicBool>,
}

impl States {
    pub(crate) fn new(motion_controlled: bool, client_controlled: bool) -> Self {
        let motion = match motion_controlled {
            true => MotionState::Still,
            false => MotionState::Ignore,
        };
        let client = match client_controlled {
            true => RtspState::Disconnected,
            false => RtspState::Ignore,
        };

        Self {
            motion: Arc::new(Mutex::new(motion)),
            client: Arc::new(Mutex::new(client)),
            live: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) fn new_from_camera_config(camera_config: &CameraConfig) -> Self {
        let motion_controlled = camera_config.pause.on_motion;
        let client_controlled = camera_config.pause.on_disconnect;

        Self::new(motion_controlled, client_controlled)
    }

    pub(crate) fn should_stream(&self) -> bool {
        if let MotionState::Still = *self.motion.lock().unwrap() {
            return false;
        }

        !matches!(*self.client.lock().unwrap(), RtspState::Disconnected)
    }

    pub(crate) fn set_client_connected(&self, value: bool) {
        let mut client = self.client.lock().unwrap();
        if let RtspState::Ignore = *client {
            return;
        }
        debug!("Client state: {}", value);
        *client = match value {
            true => RtspState::Connected,
            false => RtspState::Disconnected,
        }
    }

    pub(crate) fn set_motion_detected(&self, value: bool) {
        let mut motion = self.motion.lock().unwrap();
        if let MotionState::Ignore = *motion {
            return;
        }
        debug!("Motion state: {}", value);
        *motion = match value {
            true => MotionState::Motion,
            false => MotionState::Still,
        };
    }

    pub(crate) fn abort(&self) {
        self.live.store(false, Ordering::Relaxed);
    }

    pub(crate) fn is_live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }
}

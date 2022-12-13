//! Handles the camera in it's different states
//!

use anyhow::{anyhow, Context, Error, Result};
use log::*;
use std::collections::HashSet;
use std::sync::Arc;

use neolink_core::bc_protocol::{MotionData, Stream};

use crate::config::{CameraConfig, UserConfig};
use crate::rtsp::gst::RtspServer;
use crate::utils::AddressOrUid;

mod connected;
mod loggedin;
mod paused;
mod shared;
mod streaming;

pub(crate) use connected::Connected;
pub(crate) use loggedin::LoggedIn;
pub(crate) use paused::Paused;
pub(crate) use shared::{CameraState, Shared};
pub(crate) use streaming::Streaming;

pub(crate) enum StateInfo {
    Connected,
    LoggedIn,
    Streaming,
    Paused,
}

enum State {
    Connected(Connected),
    LoggedIn(LoggedIn),
    Streaming(Streaming),
    Paused(Paused),
}

///
/// The state machine representation of the camera
///
pub(crate) struct RtspCamera {
    shared: Shared,
    state: State,
}

impl RtspCamera {
    pub(crate) fn new(
        config: &CameraConfig,
        users: &[UserConfig],
        rtsp: Arc<RtspServer>,
    ) -> Result<Self, Error> {
        let camera_addr =
            AddressOrUid::new(&config.camera_addr, &config.camera_uid).with_context(|| {
                format!(
                    "Could not connect to camera {}. Please check UUID or IP:Port in the config",
                    config.name
                )
            })?;

        info!("{}: Connecting to camera at {}", config.name, camera_addr);
        let camera = camera_addr
            .connect_camera(config.channel_id)
            .with_context(|| {
                format!(
                    "Failed to connect to camera {} at {} on channel {}",
                    config.name, camera_addr, config.channel_id
                )
            })?;

        let mut streams: HashSet<Stream> = Default::default();
        if ["all", "both", "mainStream", "mainstream"]
            .iter()
            .any(|&e| e == config.stream)
        {
            streams.insert(Stream::Main);
        }
        if ["all", "both", "subStream", "substream"]
            .iter()
            .any(|&e| e == config.stream)
        {
            streams.insert(Stream::Sub);
        }
        if ["both", "externStream", "externstream"]
            .iter()
            .any(|&e| e == config.stream)
        {
            streams.insert(Stream::Extern);
        }

        // Used to build the list of strings to pass to gtreamer for the allowed users
        let all_users_hash = || users.iter().map(|u| u.name.clone()).collect();
        let permitted_users = match &config.permitted_users {
            // If in the camera config there is the user "anyone", or if none is specified but users
            // are defined at all, then we add all users to the camera's allowed list.
            Some(p) if p.iter().any(|u| u == "anyone") => all_users_hash(),
            None if !users.is_empty() => all_users_hash(),

            // The user specified permitted_users
            Some(p) => p.iter().cloned().collect(),

            // The user didn't specify permitted_users, and there are none defined anyway
            None => ["anonymous".to_string()].iter().cloned().collect(),
        };

        let shared = Shared {
            camera: Arc::new(camera),
            addr: camera_addr,
            name: config.name.clone(),
            channel: config.channel_id,
            username: config.username.clone(),
            password: config.password.clone(),
            pause: config.pause.clone(),
            streams,
            rtsp,
            permitted_users,
        };

        let mut state: Connected = Default::default();
        state.setup(&shared)?;

        Ok(Self {
            shared,
            state: State::Connected(state),
        })
    }

    pub(crate) fn get_state(&self) -> StateInfo {
        match &self.state {
            State::Connected(_) => StateInfo::Connected,
            State::LoggedIn(_) => StateInfo::LoggedIn,
            State::Streaming(_) => StateInfo::Streaming,
            State::Paused(_) => StateInfo::Paused,
        }
    }
    pub(crate) fn login(&mut self) -> Result<()> {
        match &mut self.state {
            State::Connected(ref mut connected) => {
                info!("{}: Logging in", &self.shared.name);
                connected.tear_down(&self.shared)?;
                let mut state: LoggedIn = Default::default();
                state.setup(&self.shared)?;
                self.state = State::LoggedIn(state);
                info!("{}: Successfully logged in", &self.shared.name);
            }
            State::Streaming(ref mut connected) => {
                info!("{}: Logging in", &self.shared.name);
                connected.tear_down(&self.shared)?;
                let mut state: LoggedIn = Default::default();
                state.setup(&self.shared)?;
                self.state = State::LoggedIn(state);
                info!("{}: Successfully stopped straming", &self.shared.name);
            }
            State::Paused(ref mut connected) => {
                info!("{}: Logging in", &self.shared.name);
                connected.tear_down(&self.shared)?;
                let mut state: LoggedIn = Default::default();
                state.setup(&self.shared)?;
                self.state = State::LoggedIn(state);
                info!(
                    "{}: Successfully stopped paused streaming",
                    &self.shared.name
                );
            }
            State::LoggedIn(_) => {
                info!("Already logged in");
            }
        }
        Ok(())
    }

    pub(crate) fn stream(&mut self) -> Result<()> {
        match &mut self.state {
            State::Connected(_) => {
                return Err(anyhow!("{}: Must login first", &self.shared.name));
            }
            State::Streaming(_) => {
                info!("{}: Already streaming", &self.shared.name);
            }
            State::Paused(ref mut paused) => {
                info!("{}: Resuming stream", &self.shared.name);
                let outputs = paused.take_outputs()?;

                paused.tear_down(&self.shared)?;
                let mut state: Streaming = Default::default();

                state.insert_outputs(outputs)?;

                state.setup(&self.shared)?;
                self.state = State::Streaming(state);
                info!("{}: Successfully resumed streaming", &self.shared.name);
            }
            State::LoggedIn(ref mut loggedin) => {
                info!("{}: Starting stream", &self.shared.name);
                loggedin.tear_down(&self.shared)?;
                let mut state: Streaming = Default::default();
                state.setup(&self.shared)?;
                self.state = State::Streaming(state);
                info!("{}: Successfully started streaming", &self.shared.name);
            }
        }
        Ok(())
    }

    pub(crate) fn pause(&mut self) -> Result<()> {
        match &mut self.state {
            State::Connected(_) => {
                return Err(anyhow!("{}: Must login first", &self.shared.name));
            }
            State::Streaming(ref mut old_state) => {
                info!("{}: Pausing stream", &self.shared.name);
                let outputs = old_state.take_outputs()?;

                old_state.tear_down(&self.shared)?;
                let mut state: Paused = Default::default();

                state.insert_outputs(outputs)?;

                state.setup(&self.shared)?;
                self.state = State::Paused(state);
                info!("{}: Successfully paused streaming", &self.shared.name);
            }
            State::Paused(_) => {
                info!("{}: Already paused", &self.shared.name);
            }
            State::LoggedIn(_) => {
                info!(
                    "{}: Cannot pause a stream that has not started",
                    &self.shared.name
                );
            }
        }
        Ok(())
    }

    pub(crate) fn client_connected(&self) -> Option<bool> {
        match &self.state {
            State::Streaming(state) => Some(state.client_connected()),
            State::Paused(state) => Some(state.client_connected()),
            _ => None,
        }
    }

    pub(crate) fn motion_data(&self) -> Result<MotionData> {
        self.shared
            .camera
            .listen_on_motion()
            .with_context(|| "Cannot get motion data")
    }

    pub(crate) fn is_running(&self) -> bool {
        match &self.state {
            State::Streaming(state) => state.is_running(),
            State::Paused(state) => state.is_running(),
            _ => true,
        }
    }

    pub(crate) fn can_pause(&self) -> bool {
        match &self.state {
            State::Streaming(state) => state.can_pause(),
            _ => false,
        }
    }

    pub(crate) fn manage(&self) -> Result<()> {
        if let State::Connected(_) = self.state {
            return Err(anyhow!("Cannot manage a camera that is not logged in"));
        }
        let cam_time = self.shared.camera.get_time()?;
        if let Some(time) = cam_time {
            info!("{}: Camera time is already set: {}", self.shared.name, time);
        } else {
            use time::OffsetDateTime;
            // We'd like now_local() but it's deprecated - try to get the local time, but if no
            // time zone, fall back to UTC.
            let new_time =
                OffsetDateTime::try_now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());

            warn!(
                "{}: Camera has no time set, setting to {}",
                self.shared.name, new_time
            );
            self.shared.camera.set_time(new_time)?;
            let cam_time = self.shared.camera.get_time()?;
            if let Some(time) = cam_time {
                info!("{}: Camera time is now set: {}", self.shared.name, time);
            } else {
                error!(
                    "{}: Camera did not accept new time (is {} an admin?)",
                    self.shared.name, self.shared.username
                );
            }
        }

        use neolink_core::bc::xml::VersionInfo;
        if let Ok(VersionInfo {
            firmwareVersion: firmware_version,
            ..
        }) = self.shared.camera.version()
        {
            info!(
                "{}: Camera reports firmware version {}",
                self.shared.name, firmware_version
            );
        } else {
            info!("{}: Could not fetch version information", self.shared.name);
        }

        Ok(())
    }
}

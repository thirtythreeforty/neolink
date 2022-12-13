use super::App;
use crate::config::CameraConfig;
use crate::utils::AddressOrUid;
use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use log::*;
use neolink_core::bc_protocol::{BcCamera, Error as BcError, LightState, MotionStatus};
use std::sync::{Arc, Mutex};

pub(crate) enum Messages {
    None,
    Login,
    MotionStop,
    MotionStart,
    Reboot,
    StatusLedOn,
    StatusLedOff,
    IRLedOn,
    IRLedOff,
    IRLedAuto,
}

pub(crate) struct EventCam<'a> {
    config: &'a CameraConfig,
    app: Arc<App>,
    channel_out: Arc<Mutex<Option<Receiver<Messages>>>>,
    channel_in: Arc<Mutex<Option<Sender<Messages>>>>,
}

impl<'a> EventCam<'a> {
    pub(crate) fn new(config: &'a CameraConfig, app: Arc<App>) -> Self {
        Self {
            config,
            app,
            channel_out: Arc::new(Mutex::new(None)),
            channel_in: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn poll(&self) -> Result<Messages> {
        if let Ok(ref mut channel_out) = self.channel_out.try_lock() {
            if let Some(channel_out) = channel_out.as_mut() {
                channel_out.recv().context("Camera failed to poll")
            } else {
                Ok(Messages::None)
            }
        } else {
            Ok(Messages::None)
        }
    }

    pub(crate) fn send_message(&self, msg: Messages) -> Result<()> {
        if let Ok(ref mut channel_in) = self.channel_in.lock() {
            if let Some(channel_in) = channel_in.as_mut() {
                channel_in
                    .send(msg)
                    .context("Failed to send message from camera")
            } else {
                Err(anyhow!("Failed to send camera data over crossbeam"))
            }
        } else {
            Err(anyhow!("Failed to lock"))
        }
    }

    pub(crate) fn abort(&self) {
        self.app.abort(&self.config.name);
    }

    pub(crate) fn start_listening(&self) {
        let loop_config = self.config;

        // Channels from the camera
        let (tx, rx) = unbounded();
        let loop_tx = Arc::new(Mutex::new(tx));
        *self.channel_out.lock().unwrap() = Some(rx);

        // Channels to the camera
        let (stx, srx) = unbounded();
        let loop_rx = Arc::new(Mutex::new(srx));
        *self.channel_in.lock().unwrap() = Some(stx);

        while self.app.running(&self.config.name) {
            // Ignore errors and just loop
            let _ = Self::cam_run(
                loop_config,
                loop_tx.clone(),
                loop_rx.clone(),
                self.app.clone(),
            );
        }
    }

    fn cam_run(
        camera_config: &CameraConfig,
        tx: Arc<Mutex<Sender<Messages>>>,
        rx: Arc<Mutex<Receiver<Messages>>>,
        app: Arc<App>,
    ) -> Result<()> {
        let camera_addr =
            AddressOrUid::new(&camera_config.camera_addr, &camera_config.camera_uid).unwrap();

        info!(
            "{}: Connecting to camera at {}",
            camera_config.name, camera_addr
        );
        let camera = camera_addr
            .connect_camera(camera_config.channel_id)
            .with_context(|| {
                format!(
                    "Failed to connect to camera {} at {} on channel {}",
                    camera_config.name, camera_addr, camera_config.channel_id
                )
            })?;

        info!("{}: Logging in", camera_config.name);
        camera
            .login(&camera_config.username, camera_config.password.as_deref())
            .context("Failed to login to the camera")?;
        info!("{}: Connected and logged in", camera_config.name);

        (tx.lock().map_err(|_| anyhow!("Failed to lock"))?).send(Messages::Login)?;

        // Shararble cameras
        let arc_cam = Arc::new(camera);

        let mut motion_thread = MotionThread {
            app: app.clone(),
            tx,
            camera: arc_cam.clone(),
            name: camera_config.name.to_string(),
        };

        let mut message_handler = MessageHandler {
            app: app.clone(),
            rx,
            camera: arc_cam,
            name: camera_config.name.to_string(),
        };

        let _ = crossbeam::scope(|s| {
            info!("{}: Listening to Camera Motion", camera_config.name);
            s.spawn(|_| {
                if let Err(e) = motion_thread.run() {
                    error!("Motion thread aborted: {:?}", e);
                }
                app.abort(&camera_config.name);
            });

            info!("{}: Setting up camera actions", camera_config.name);
            s.spawn(|_| {
                message_handler.listen();
            });
        });

        Ok(())
    }
}

struct MotionThread {
    app: Arc<App>,
    tx: Arc<Mutex<Sender<Messages>>>,
    camera: Arc<BcCamera>,
    name: String,
}

impl MotionThread {
    fn run(&mut self) -> Result<()> {
        let mut motion_data = self.camera.listen_on_motion()?;
        while self.app.running(&format!("app:{}", self.name)) {
            for motion_status in motion_data.consume_motion_events()?.drain(..) {
                match motion_status {
                    MotionStatus::Start(_) => {
                        (self
                            .tx
                            .lock()
                            .map_err(|_| BcError::Other("Failed to lock mutex"))?)
                        .send(Messages::MotionStart)
                        .map_err(|_| BcError::Other("Failed to send Message"))?;
                    }
                    MotionStatus::Stop(_) => {
                        (self
                            .tx
                            .lock()
                            .map_err(|_| BcError::Other("Failed to lock mutex"))?)
                        .send(Messages::MotionStop)
                        .map_err(|_| BcError::Other("Failed to send Message"))?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

struct MessageHandler {
    app: Arc<App>,
    rx: Arc<Mutex<Receiver<Messages>>>,
    camera: Arc<BcCamera>,
    name: String,
}

impl MessageHandler {
    fn listen(&mut self) {
        while self.app.running(&format!("app:{}", self.name)) {
            // Try and lock don't worry if not
            if let Ok(ref mut channel_out) = self.rx.try_lock() {
                match channel_out.try_recv() {
                    Ok(message) => match message {
                        Messages::Reboot => {
                            if self.camera.reboot().is_err() {
                                error!("Failed to reboot the camera");
                                self.abort()
                            }
                        }
                        Messages::StatusLedOn => {
                            if self.camera.led_light_set(true).is_err() {
                                error!("Failed to turn on the status light");
                                self.abort();
                            }
                        }
                        Messages::StatusLedOff => {
                            if self.camera.led_light_set(false).is_err() {
                                error!("Failed to turn off the status light");
                                self.abort();
                            }
                        }
                        Messages::IRLedOn => {
                            if self.camera.irled_light_set(LightState::On).is_err() {
                                error!("Failed to turn on the status light");
                                self.abort();
                            }
                        }
                        Messages::IRLedOff => {
                            if self.camera.irled_light_set(LightState::Off).is_err() {
                                error!("Failed to turn on the status light");
                                self.abort();
                            }
                        }
                        Messages::IRLedAuto => {
                            if self.camera.irled_light_set(LightState::Auto).is_err() {
                                error!("Failed to turn on the status light");
                                self.abort();
                            }
                        }
                        _ => {}
                    },
                    Err(TryRecvError::Empty) => (),
                    Err(TryRecvError::Disconnected) => self.abort(),
                }
            }
        }
    }

    fn abort(&self) {
        self.app.abort(&self.name);
    }
}

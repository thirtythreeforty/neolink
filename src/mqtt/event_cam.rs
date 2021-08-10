use super::App;
use super::{config::CameraConfig, errors::Error};
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use log::*;
use neolink_core::bc_protocol::{
    BcCamera, Error as BcError, LightState, MotionOutput, MotionOutputError, MotionStatus,
};
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

    pub(crate) fn poll(&self) -> Result<Messages, Error> {
        if let Ok(ref mut channel_out) = self.channel_out.try_lock() {
            if let Some(channel_out) = channel_out.as_mut() {
                channel_out.recv().map_err(|e| e.into())
            } else {
                Ok(Messages::None)
            }
        } else {
            Ok(Messages::None)
        }
    }

    pub(crate) fn send_message(&self, msg: Messages) -> Result<(), Error> {
        if let Ok(ref mut channel_in) = self.channel_in.lock() {
            if let Some(channel_in) = channel_in.as_mut() {
                channel_in.send(msg).map_err(|e| e.into())
            } else {
                Err(Error::CrossbeamSend)
            }
        } else {
            Err(Error::Lock)
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
                &loop_config,
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
    ) -> Result<(), Error> {
        info!(
            "{}: Connecting to camera at {}",
            camera_config.name, camera_config.camera_addr
        );

        let mut camera =
            BcCamera::new_with_addr(&camera_config.camera_addr, camera_config.channel_id)?;

        info!("{}: Logging in", camera_config.name);
        camera.login(&camera_config.username, camera_config.password.as_deref())?;
        info!("{}: Connected and logged in", camera_config.name);

        (tx.lock()?).send(Messages::Login)?;

        // Shararble cameras
        let arc_cam = Arc::new(camera);
        let cam_motion = arc_cam.clone();
        let cam_mesg = arc_cam;

        let mut motion_callback = MotionCallback {
            app: app.clone(),
            tx,
            name: camera_config.name.to_string(),
        };

        let mut message_handler = MessageHandler {
            app: app.clone(),
            rx,
            camera: cam_mesg,
            name: camera_config.name.to_string(),
        };

        let _ = crossbeam::scope(|s| {
            info!("{}: Listening to Camera Motion", camera_config.name);
            s.spawn(|_| {
                let _ = cam_motion.listen_on_motion(&mut motion_callback);
                // If listen_on_motion returns then camera disconnect
                app.abort(&camera_config.name);
            });

            info!("{}: Setting up camera actions", camera_config.name);
            s.spawn(|_| {
                let _ = message_handler.listen();
            });
        });

        Ok(())
    }
}

struct MotionCallback {
    app: Arc<App>,
    tx: Arc<Mutex<Sender<Messages>>>,
    name: String,
}

impl MotionOutput for MotionCallback {
    fn motion_recv(&mut self, motion_status: MotionStatus) -> MotionOutputError {
        if self.app.running(&format!("app:{}", self.name)) {
            match motion_status {
                MotionStatus::Start => {
                    (self
                        .tx
                        .lock()
                        .map_err(|_| BcError::Other("Failed to lock mutex"))?)
                    .send(Messages::MotionStart)
                    .map_err(|_| BcError::Other("Failed to send Message"))?;
                }
                MotionStatus::Stop => {
                    (self
                        .tx
                        .lock()
                        .map_err(|_| BcError::Other("Failed to lock mutex"))?)
                    .send(Messages::MotionStop)
                    .map_err(|_| BcError::Other("Failed to send Message"))?;
                }
                _ => {}
            }
            Ok(true)
        } else {
            Ok(false)
        }
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
        if self.app.running(&format!("app:{}", self.name)) {
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

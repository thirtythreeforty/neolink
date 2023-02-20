use super::App;
use crate::config::CameraConfig;
use crate::utils::AddressOrUid;
use anyhow::{anyhow, Context, Result};
use log::*;
use neolink_core::bc_protocol::{
    BcCamera, Direction as BcDirection, Error as BcError, LightState, MotionStatus,
};
use std::sync::Arc;
use tokio::sync::{
    mpsc::{channel, error::TryRecvError, Receiver, Sender},
    Mutex,
};

#[derive(Debug, Copy, Clone)]
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
    Battery,
    Ptz(Direction),
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum Direction {
    Up(f32),
    Down(f32),
    Left(f32),
    Right(f32),
    In(f32),
    Out(f32),
}

#[derive(Debug)]
enum ToCamera {
    Send(Messages),
    SendAndReply {
        message: Messages,
        reply: Sender<String>,
    },
}

pub(crate) struct EventCam {
    config: Arc<CameraConfig>,
    app: Arc<App>,
    channel_out: Arc<Mutex<Option<Receiver<Messages>>>>,
    channel_in: Arc<Mutex<Option<Sender<ToCamera>>>>,
}

impl EventCam {
    pub(crate) fn new(config: Arc<CameraConfig>, app: Arc<App>) -> Self {
        Self {
            config,
            app,
            channel_out: Arc::new(Mutex::new(None)),
            channel_in: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) async fn poll(&self) -> Result<Messages> {
        if let Ok(ref mut channel_out) = self.channel_out.try_lock() {
            if let Some(channel_out) = channel_out.as_mut() {
                channel_out.recv().await.context("Camera failed to poll")
            } else {
                Ok(Messages::None)
            }
        } else {
            Ok(Messages::None)
        }
    }

    pub(crate) async fn send_message(&self, msg: Messages) -> Result<()> {
        let mut channel_in = self.channel_in.lock().await;
        if let Some(channel_in) = channel_in.as_mut() {
            channel_in
                .send(ToCamera::Send(msg))
                .await
                .context("Failed to send message from camera")
        } else {
            Err(anyhow!("Failed to send camera data over crossbeam"))
        }
    }

    pub(crate) async fn send_message_with_reply(&self, msg: Messages) -> Result<String> {
        let mut channel_in = self.channel_in.lock().await;
        if let Some(channel_in) = channel_in.as_mut() {
            let (tx, mut rx) = channel(1);
            channel_in
                .send(ToCamera::SendAndReply {
                    message: msg,
                    reply: tx,
                })
                .await
                .context("Failed to send message from camera")?;
            tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
                .await
                .context("Failed to recieve reply Timeout")?
                .ok_or_else(|| anyhow!("Failed to recieve reply"))
        } else {
            Err(anyhow!("Failed to send camera data over crossbeam"))
        }
    }

    pub(crate) fn abort(&self) {
        self.app.abort(&self.config.name);
    }

    pub(crate) async fn start_listening(&self) {
        let loop_config = self.config.clone();

        // Channels from the camera
        let (tx, rx) = channel(40);
        let loop_tx = Arc::new(Mutex::new(tx));
        *self.channel_out.lock().await = Some(rx);

        // Channels to the camera
        let (stx, srx) = channel(40);
        let loop_rx = Arc::new(Mutex::new(srx));
        *self.channel_in.lock().await = Some(stx);

        while self.app.running(&self.config.name) {
            // Ignore errors and just loop
            let _ = Self::cam_run(
                &loop_config,
                loop_tx.clone(),
                loop_rx.clone(),
                self.app.clone(),
            )
            .await;
        }
    }

    async fn cam_run(
        camera_config: &CameraConfig,
        tx: Arc<Mutex<Sender<Messages>>>,
        rx: Arc<Mutex<Receiver<ToCamera>>>,
        app: Arc<App>,
    ) -> Result<()> {
        let camera_addr = AddressOrUid::new(
            &camera_config.camera_addr,
            &camera_config.camera_uid,
            &camera_config.discovery,
        )
        .unwrap();

        info!(
            "{}: Connecting to camera at {}",
            camera_config.name, camera_addr
        );
        let camera = camera_addr
            .connect_camera(
                camera_config.channel_id,
                &camera_config.username,
                camera_config.password.as_ref(),
            )
            .await
            .with_context(|| {
                format!(
                    "Failed to connect to camera {} at {} on channel {}",
                    camera_config.name, camera_addr, camera_config.channel_id
                )
            })?;

        info!("{}: Logging in", camera_config.name);
        camera
            .login()
            .await
            .context("Failed to login to the camera")?;
        info!("{}: Connected and logged in", camera_config.name);

        (tx.lock().await).send(Messages::Login).await?;

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

        let camera_name = camera_config.name.clone();
        info!("{}: Listening to Camera Motion", camera_config.name);
        let handle_motion = tokio::task::spawn(async move {
            if let Err(e) = motion_thread.run().await {
                error!("Motion thread aborted: {:?}", e);
            }
            app.abort(&camera_name);
        });
        info!("{}: Setting up camera actions", camera_config.name);
        let handle_camera = tokio::task::spawn(async move {
            message_handler.listen().await;
        });

        tokio::select! {
            val = async {handle_motion.await} => {val},
            val = async {handle_camera.await} => {val}
        }?;

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
    async fn run(&mut self) -> Result<()> {
        let mut motion_data = self.camera.listen_on_motion().await?;
        while self.app.running(&format!("app:{}", self.name)) {
            for motion_status in motion_data.consume_motion_events()?.drain(..) {
                match motion_status {
                    MotionStatus::Start(_) => {
                        (self.tx.lock().await)
                            .send(Messages::MotionStart)
                            .await
                            .map_err(|_| BcError::Other("Failed to send Message"))?;
                    }
                    MotionStatus::Stop(_) => {
                        (self.tx.lock().await)
                            .send(Messages::MotionStop)
                            .await
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
    rx: Arc<Mutex<Receiver<ToCamera>>>,
    camera: Arc<BcCamera>,
    name: String,
}

impl MessageHandler {
    async fn listen(&mut self) {
        while self.app.running(&format!("app:{}", self.name)) {
            // Try and lock don't worry if not
            if let Ok(ref mut channel_out) = self.rx.try_lock() {
                match channel_out.try_recv() {
                    Ok(to_camera) => {
                        let (replier, message) = match to_camera {
                            ToCamera::Send(message) => (None, message),
                            ToCamera::SendAndReply { message, reply } => (Some(reply), message),
                        };
                        let reply = match message {
                            Messages::Reboot => {
                                if self.camera.reboot().await.is_err() {
                                    error!("Failed to reboot the camera");
                                    self.abort();
                                    "FAIL".to_string()
                                } else {
                                    "OK".to_string()
                                }
                            }
                            Messages::StatusLedOn => {
                                if self.camera.led_light_set(true).await.is_err() {
                                    error!("Failed to turn on the status light");
                                    self.abort();
                                    "FAIL".to_string()
                                } else {
                                    "OK".to_string()
                                }
                            }
                            Messages::StatusLedOff => {
                                if self.camera.led_light_set(false).await.is_err() {
                                    error!("Failed to turn off the status light");
                                    self.abort();
                                    "FAIL".to_string()
                                } else {
                                    "OK".to_string()
                                }
                            }
                            Messages::IRLedOn => {
                                if self.camera.irled_light_set(LightState::On).await.is_err() {
                                    error!("Failed to turn on the status light");
                                    self.abort();
                                    "FAIL".to_string()
                                } else {
                                    "OK".to_string()
                                }
                            }
                            Messages::IRLedOff => {
                                if self.camera.irled_light_set(LightState::Off).await.is_err() {
                                    error!("Failed to turn on the status light");
                                    self.abort();
                                    "FAIL".to_string()
                                } else {
                                    "OK".to_string()
                                }
                            }
                            Messages::IRLedAuto => {
                                if self.camera.irled_light_set(LightState::Auto).await.is_err() {
                                    error!("Failed to turn on the status light");
                                    self.abort();
                                    "FAIL".to_string()
                                } else {
                                    "OK".to_string()
                                }
                            }
                            Messages::Battery => {
                                todo!()
                            }
                            Messages::Ptz(direction) => {
                                let (bc_direction, amount) = match direction {
                                    Direction::Up(amount) => (BcDirection::Up, amount),
                                    Direction::Down(amount) => (BcDirection::Down, amount),
                                    Direction::Left(amount) => (BcDirection::Left, amount),
                                    Direction::Right(amount) => (BcDirection::Right, amount),
                                    Direction::In(amount) => (BcDirection::In, amount),
                                    Direction::Out(amount) => (BcDirection::Out, amount),
                                };
                                if self.camera.send_ptz(bc_direction, amount).await.is_err() {
                                    error!("Failed to turn on the status light");
                                    self.abort();
                                    "FAIL".to_string()
                                } else {
                                    "OK".to_string()
                                }
                            }
                            _ => "UNKNOWN COMMAND".to_string(),
                        };
                        if let Some(replier) = replier {
                            let _ = replier.send(reply).await;
                        }
                    }
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

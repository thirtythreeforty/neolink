use crate::config::CameraConfig;
use crate::utils::AddressOrUid;
use anyhow::{anyhow, Context, Result};
use log::*;
use neolink_core::bc_protocol::{
    BcCamera, Direction as BcDirection, Error as BcError, LightState, MaxEncryption, MotionStatus,
};
use std::sync::Arc;
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinSet,
};

#[derive(Debug, Copy, Clone)]
pub(crate) enum Messages {
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
    PIROn,
    PIROff,
    PIRQuery,
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
    Stop(f32)
}

#[derive(Debug)]
enum ToCamera {
    Send(Messages),
    SendAndReply {
        message: Messages,
        reply: Sender<String>,
    },
}

#[derive(Clone)]
pub(crate) struct EventCamSender {
    channel_in: Sender<ToCamera>,
}

impl EventCamSender {
    #[allow(dead_code)]
    pub(crate) async fn send_message(&self, msg: Messages) -> Result<()> {
        self.channel_in
            .send(ToCamera::Send(msg))
            .await
            .context("Failed to send message from camera")
    }

    pub(crate) async fn send_message_with_reply(&self, msg: Messages) -> Result<String> {
        let (tx, mut rx) = channel(1);
        self.channel_in
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
    }
}
pub(crate) struct EventCam {
    sender: EventCamSender,
    channel_out: Receiver<Messages>,
    set: JoinSet<Result<()>>,
}

impl EventCam {
    pub(crate) async fn new(config: Arc<CameraConfig>) -> Self {
        // Channels from the camera
        let (tx, rx) = channel(40);

        // Channels to the camera
        let (stx, srx) = channel(40);

        let mut set = JoinSet::<Result<()>>::new();
        let thread_config = config;
        let mut eventcam_thread = EventCamThread { rx: srx, tx };
        set.spawn(async move {
            let mut wait_for = tokio::time::Duration::from_micros(125);
            loop {
                // Ignore errors and just loop
                tokio::task::yield_now().await;
                if let Err(e) = eventcam_thread.cam_run(&thread_config).await {
                    warn!("Camera thread error: {:?}. Restarting...", e);
                    tokio::time::sleep(wait_for).await;
                    wait_for *= 2;
                }
            }
        });

        Self {
            sender: EventCamSender { channel_in: stx },
            channel_out: rx,
            set,
        }
    }

    /// This will also error is the join set errors
    pub(crate) async fn poll(&mut self) -> Result<Messages> {
        let (incoming, set) = (&mut self.channel_out, &mut self.set);
        tokio::select! {
            v = incoming.recv() => v.with_context(|| "Camera Polling error"),
            v = async {
                while let Some(res) = set.join_next().await {
                    match res {
                        Err(e) => {
                            set.abort_all();
                            return Err(e.into());
                        }
                        Ok(Err(e)) => {
                            set.abort_all();
                            return Err(e);
                        }
                        Ok(Ok(())) => {}
                    }
                }
                Err(anyhow!("Camera background thread dropped without error"))
            } => v.with_context(|| "Camera Threads aborted"),
        }
    }

    pub(crate) fn get_sender(&self) -> EventCamSender {
        self.sender.clone()
    }
}

impl Drop for EventCam {
    fn drop(&mut self) {
        self.set.abort_all();
    }
}

struct EventCamThread {
    rx: Receiver<ToCamera>,
    tx: Sender<Messages>,
}

impl EventCamThread {
    async fn cam_run(&mut self, camera_config: &CameraConfig) -> Result<()> {
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
            .connect_camera(camera_config)
            .await
            .with_context(|| {
                format!(
                    "Failed to connect to camera {} at {} on channel {}",
                    camera_config.name, camera_addr, camera_config.channel_id
                )
            })?;

        info!("{}: Logging in", camera_config.name);
        let max_encryption = match camera_config.max_encryption.to_lowercase().as_str() {
            "none" => MaxEncryption::None,
            "bcencrypt" => MaxEncryption::BcEncrypt,
            "aes" => MaxEncryption::Aes,
            _ => MaxEncryption::Aes,
        };
        camera
            .login_with_maxenc(max_encryption)
            .await
            .context("Failed to login to the camera")?;
        info!("{}: Connected and logged in", camera_config.name);

        self.tx.send(Messages::Login).await?;

        // Shararble cameras
        let arc_cam = Arc::new(camera);

        let mut motion_thread = MotionThread {
            tx: self.tx.clone(),
            camera: arc_cam.clone(),
        };

        let mut message_handler = MessageHandler {
            rx: &mut self.rx,
            camera: arc_cam,
        };

        tokio::select! {
            val = async {
                info!("{}: Listening to Camera Motion", camera_config.name);
                motion_thread.run().await
            } => {
                if let Err(e) = val {
                    error!("Motion thread aborted: {:?}", e);
                    Err(e)
                } else {
                    debug!("Normal finish on motion thread");
                    Ok(())
                }
            },
            val = async {
                info!("{}: Setting up camera actions", camera_config.name);
                message_handler.listen().await
            } => {
                if let Err(e) = val {
                    error!("Camera message handler aborted: {:?}", e);
                    Err(e)
                } else {
                    debug!("Normal finish on camera message thread");
                    Ok(())
                }
            },
        }?;

        Ok(())
    }
}

struct MotionThread {
    tx: Sender<Messages>,
    camera: Arc<BcCamera>,
}

impl MotionThread {
    async fn run(&mut self) -> Result<()> {
        let mut motion_data = self.camera.listen_on_motion().await?;
        let mut queued_motion = motion_data.consume_motion_events()?;
        loop {
            tokio::task::yield_now().await;
            for motion_status in queued_motion.drain(..) {
                match motion_status {
                    MotionStatus::Start(_) => {
                        self.tx
                            .send(Messages::MotionStart)
                            .await
                            .map_err(|_| BcError::Other("Failed to send Message"))?;
                    }
                    MotionStatus::Stop(_) => {
                        self.tx
                            .send(Messages::MotionStop)
                            .await
                            .map_err(|_| BcError::Other("Failed to send Message"))?;
                    }
                    _ => {}
                }
            }
            queued_motion.push(motion_data.next_motion().await?);
        }
    }
}

struct MessageHandler<'a> {
    rx: &'a mut Receiver<ToCamera>,
    camera: Arc<BcCamera>,
}

impl<'a> MessageHandler<'a> {
    async fn listen(&mut self) -> Result<()> {
        loop {
            tokio::task::yield_now().await;
            match self.rx.recv().await {
                Some(to_camera) => {
                    let (replier, message) = match to_camera {
                        ToCamera::Send(message) => (None, message),
                        ToCamera::SendAndReply { message, reply } => (Some(reply), message),
                    };
                    let mut error = None;
                    let reply = match message {
                        Messages::Reboot => {
                            if let Err(e) = self.camera.reboot().await {
                                error = Some(format!("Failed to reboot the camera: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::StatusLedOn => {
                            if let Err(e) = self.camera.led_light_set(true).await {
                                error =
                                    Some(format!("Failed to turn on the status light: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::StatusLedOff => {
                            if let Err(e) = self.camera.led_light_set(false).await {
                                error =
                                    Some(format!("Failed to turn off the status light: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::IRLedOn => {
                            if let Err(e) = self.camera.irled_light_set(LightState::On).await {
                                error =
                                    Some(format!("Failed to turn on the status light: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::IRLedOff => {
                            if let Err(e) = self.camera.irled_light_set(LightState::Off).await {
                                error =
                                    Some(format!("Failed to turn on the status light: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::IRLedAuto => {
                            if let Err(e) = self.camera.irled_light_set(LightState::Auto).await {
                                error =
                                    Some(format!("Failed to turn on the status light: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::Battery => match self.camera.battery_info().await {
                            Err(e) => {
                                error!("Failed to get battery status: {:?}", e);
                                "FAIL".to_string()
                            }
                            Ok(battery_info) => {
                                let bytes_res = yaserde::ser::serialize_with_writer(
                                    &battery_info,
                                    vec![],
                                    &Default::default(),
                                );
                                match bytes_res {
                                    Ok(bytes) => match String::from_utf8(bytes) {
                                        Ok(str) => str,
                                        Err(_) => {
                                            error!("Failed to encode battery status");
                                            "FAIL".to_string()
                                        }
                                    },
                                    Err(_) => {
                                        error!("Failed to serialise battery status");
                                        "FAIL".to_string()
                                    }
                                }
                            }
                        },
                        Messages::PIRQuery => match self.camera.get_pirstate().await {
                            Err(e) => {
                                error!("Failed to get pir status: {:?}", e);
                                "FAIL".to_string()
                            }
                            Ok(pir_info) => {
                                let bytes_res = yaserde::ser::serialize_with_writer(
                                    &pir_info,
                                    vec![],
                                    &Default::default(),
                                );
                                match bytes_res {
                                    Ok(bytes) => match String::from_utf8(bytes) {
                                        Ok(str) => str,
                                        Err(_) => {
                                            error!("Failed to encode pir status");
                                            "FAIL".to_string()
                                        }
                                    },
                                    Err(_) => {
                                        error!("Failed to serialise pir status");
                                        "FAIL".to_string()
                                    }
                                }
                            }
                        },
                        Messages::PIROn => {
                            if let Err(e) = self.camera.pir_set(true).await {
                                error = Some(format!("Failed to turn on the pir: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::PIROff => {
                            if let Err(e) = self.camera.pir_set(false).await {
                                error = Some(format!("Failed to turn on the pir: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::Ptz(direction) => {
                            let (bc_direction, amount) = match direction {
                                Direction::Up(amount) => (BcDirection::Up, amount),
                                Direction::Down(amount) => (BcDirection::Down, amount),
                                Direction::Left(amount) => (BcDirection::Left, amount),
                                Direction::Right(amount) => (BcDirection::Right, amount),
                                Direction::In(amount) => (BcDirection::In, amount),
                                Direction::Out(amount) => (BcDirection::Out, amount),
                                Direction::Stop(amount) => (BcDirection::Stop, amount)
                            };
                            if let Err(e) = self.camera.send_ptz(bc_direction, amount).await {
                                error = Some(format!("Failed to send PTZ: {:?}", e));
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
                    if let Some(error) = error {
                        return Err(anyhow!("{}", error));
                    }
                }
                None => {
                    return Err(anyhow!("Message handller channel dropped"));
                }
            }
        }
    }
}

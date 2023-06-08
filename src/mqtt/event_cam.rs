use crate::config::CameraConfig;
use crate::utils::AddressOrUid;
use anyhow::{anyhow, Context, Result};
use futures::stream::StreamExt;
use log::*;
use neolink_core::bc_protocol::{
    BcCamera, Direction as BcDirection, Error as BcError, LightState, MaxEncryption, MotionStatus,
};
use std::sync::Arc;
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinSet,
    time::{interval, sleep, Duration},
};

#[derive(Debug, Clone)]
pub(crate) enum Messages {
    Login,
    MotionStop,
    MotionStart,
    Reboot,
    FloodlightOn,
    FloodlightOff,
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
    Snap(Vec<u8>),
    BatteryLevel(u32),
    Preset(u8),
    PresetAssign(u8, String),
    PresetQuery,
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum Direction {
    Up(f32, f32),
    Down(f32, f32),
    Left(f32, f32),
    Right(f32, f32),
    In(f32, f32),
    Out(f32, f32),
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

        let mut flight_thread = FloodlightThread {
            tx: self.tx.clone(),
            camera: arc_cam.clone(),
        };

        let mut snap_thread = SnapThread {
            tx: self.tx.clone(),
            camera: arc_cam.clone(),
        };

        let mut battery_thread = BatteryLevelThread {
            tx: self.tx.clone(),
            camera: arc_cam.clone(),
        };

        let mut keepalive_thread = KeepaliveThread {
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
            }, if camera_config.mqtt.as_ref().expect("Should have an mqtt config at this point").enable_motion => {
                if let Err(e) = val {
                    error!("Motion thread aborted: {:?}", e);
                    Err(e)
                } else {
                    debug!("Normal finish on motion thread");
                    Ok(())
                }
            },
            val = async {
                debug!("{}: Starting Pings", camera_config.name);
                keepalive_thread.run().await
            }, if camera_config.mqtt.as_ref().expect("Should have an mqtt config at this point").enable_pings => {
                if let Err(e) = val {
                    debug!("Ping thread aborted: {:?}", e);
                    Err(e)
                } else {
                    debug!("Normal finish on Ping thread");
                    Ok(())
                }
            },
            val = async {
                info!("{}: Listening to FloodLight Status", camera_config.name);
                flight_thread.run().await
            }, if camera_config.mqtt.as_ref().expect("Should have an mqtt config at this point").enable_light => {
                if let Err(e) = val {
                    error!("FloodLight thread aborted: {:?}", e);
                    Err(e)
                } else {
                    debug!("Normal finish on FloodLight thread");
                    Ok(())
                }
            },
            val = async {
                info!("{}: Updating Preview", camera_config.name);
                snap_thread.run().await
            }, if camera_config.mqtt.as_ref().expect("Should have an mqtt config at this point").enable_preview => {
                if let Err(e) = val {
                    error!("Snap thread aborted: {:?}", e);
                    Err(e)
                } else {
                    debug!("Normal finish on Snap thread");
                    Ok(())
                }
            },
            val = async {
                info!("{}: Updating Battery Level", camera_config.name);
                battery_thread.run().await
            }, if camera_config.mqtt.as_ref().expect("Should have an mqtt config at this point").enable_battery => {
                if let Err(e) = val {
                    error!("Battery thread aborted: {:?}", e);
                    Err(e)
                } else {
                    debug!("Normal finish on Battery thread");
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

struct FloodlightThread {
    tx: Sender<Messages>,
    camera: Arc<BcCamera>,
}

impl FloodlightThread {
    async fn run(&mut self) -> Result<()> {
        let mut reciever =
            tokio_stream::wrappers::ReceiverStream::new(self.camera.listen_on_flightlight().await?);
        while let Some(flights) = reciever.next().await {
            for flight in flights.floodlight_status_list.iter() {
                if flight.status == 0 {
                    self.tx.send(Messages::FloodlightOff).await?;
                } else {
                    self.tx.send(Messages::FloodlightOn).await?;
                }
            }
        }
        Ok(())
    }
}

struct SnapThread {
    tx: Sender<Messages>,
    camera: Arc<BcCamera>,
}

impl SnapThread {
    async fn run(&mut self) -> Result<()> {
        let mut tries = 0;
        let base_duration = Duration::from_millis(500);
        loop {
            tokio::time::sleep(base_duration.saturating_mul(tries)).await;
            let snapshot = match self.camera.get_snapshot().await {
                Ok(info) => {
                    tries = 1;
                    info
                }
                Err(neolink_core::Error::UnintelligibleReply { reply, why }) => {
                    log::debug!("Reply: {:?}, why: {:?}", reply, why);
                    // Try again later
                    tries += 1;
                    continue;
                }
                Err(neolink_core::Error::CameraServiceUnavaliable) => {
                    log::debug!("Snap not supported");
                    futures::future::pending().await
                }
                Err(e) => return Err(e.into()),
            };
            self.tx.send(Messages::Snap(snapshot)).await?;
        }
    }
}

struct BatteryLevelThread {
    tx: Sender<Messages>,
    camera: Arc<BcCamera>,
}

impl BatteryLevelThread {
    async fn run(&mut self) -> Result<()> {
        let mut tries = 0;
        let base_duration = Duration::from_secs(15);
        loop {
            tokio::time::sleep(base_duration.saturating_mul(tries)).await;
            let battery = match self.camera.battery_info().await {
                Ok(info) => {
                    tries = 1;
                    info
                }
                Err(neolink_core::Error::UnintelligibleReply { .. }) => {
                    // Try again later
                    tries += 1;
                    continue;
                }
                Err(neolink_core::Error::CameraServiceUnavaliable) => {
                    log::debug!("Battery not supported");
                    futures::future::pending().await
                }
                Err(e) => return Err(e.into()),
            };
            self.tx
                .send(Messages::BatteryLevel(battery.battery_percent))
                .await?;
        }
    }
}

struct KeepaliveThread {
    camera: Arc<BcCamera>,
}

impl KeepaliveThread {
    async fn run(&mut self) -> Result<()> {
        let mut interval =
            tokio_stream::wrappers::IntervalStream::new(interval(Duration::from_secs(5)));
        while let Some(_update) = interval.next().await {
            if self.camera.ping().await.is_err() {
                break;
            }
        }

        futures::pending!(); // Never actually finish, has to be aborted
        Ok(())
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
                        Messages::FloodlightOn => {
                            let res = self.camera.set_floodlight_manual(true, 180).await;
                            if res.is_err() {
                                error = Some(format!(
                                    "Failed to turn on the floodlight light: {:?}",
                                    res.err()
                                ));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::FloodlightOff => {
                            let res = self.camera.set_floodlight_manual(false, 180).await;
                            if res.is_err() {
                                error = Some(format!(
                                    "Failed to turn off the floodlight light: {:?}",
                                    res.err()
                                ));
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
                            let (bc_direction, speed, seconds) = match direction {
                                Direction::Up(speed, seconds) => (BcDirection::Up, speed, seconds),
                                Direction::Down(speed, seconds) => {
                                    (BcDirection::Down, speed, seconds)
                                }
                                Direction::Left(speed, seconds) => {
                                    (BcDirection::Left, speed, seconds)
                                }
                                Direction::Right(speed, seconds) => {
                                    (BcDirection::Right, speed, seconds)
                                }
                                Direction::In(speed, seconds) => (BcDirection::In, speed, seconds),
                                Direction::Out(speed, seconds) => {
                                    (BcDirection::Out, speed, seconds)
                                }
                            };
                            if let Err(e) = self.camera.send_ptz(bc_direction, speed).await {
                                error = Some(format!("Failed to send PTZ: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                // sleep for the designated seconds
                                sleep(Duration::from_secs_f32(seconds)).await;

                                // note that amount is not used in the stop command
                                if let Err(e) = self.camera.send_ptz(BcDirection::Stop, 0.0).await {
                                    error = Some(format!("Failed to send PTZ: {:?}", e));
                                    "FAIL".to_string()
                                } else {
                                    "OK".to_string()
                                }
                            }
                        }
                        Messages::Preset(id) => {
                            if let Err(e) = self.camera.moveto_ptz_preset(id).await {
                                error = Some(format!("Failed to send PTZ preset: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::PresetAssign(id, name) => {
                            if let Err(e) = self.camera.set_ptz_preset(id, name).await {
                                error = Some(format!("Failed to send PTZ preset: {:?}", e));
                                "FAIL".to_string()
                            } else {
                                "OK".to_string()
                            }
                        }
                        Messages::PresetQuery => match self.camera.get_ptz_preset().await {
                            Err(e) => {
                                error!("Failed to get PTZ preset status: {:?}", e);
                                "FAIL".to_string()
                            }
                            Ok(ptz_info) => {
                                let bytes_res = yaserde::ser::serialize_with_writer(
                                    &ptz_info,
                                    vec![],
                                    &Default::default(),
                                );
                                match bytes_res {
                                    Ok(bytes) => match String::from_utf8(bytes) {
                                        Ok(str) => str,
                                        Err(_) => {
                                            error!("Failed to encode PTZ preset status");
                                            "FAIL".to_string()
                                        }
                                    },
                                    Err(_) => {
                                        error!("Failed to serialise PTZ preset status");
                                        "FAIL".to_string()
                                    }
                                }
                            }
                        },
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

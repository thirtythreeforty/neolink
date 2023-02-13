use log::*;
///
/// # Neolink MQTT
///
/// Handles incoming and outgoing MQTT messages
///
/// This acts as a bridge between cameras and MQTT servers
///
/// Messages are prefixed with `neolink/{CAMERANAME}`
///
/// Control messages:
///
/// - `/control/led [on|off]` Turns status LED on/off
/// - `/control/ir [on|off|auto]` Turn IR lights on/off or automatically via light detection
/// - `/control/reboot` Reboot the camera
/// - `/control/ptz` [up|down|left|right|in|out] (amount) Control the PTZ movements, amount defaults to 32.0
///
/// Status Messages:
///
/// `/status offline` Sent when the neolink goes offline this is a LastWill message
/// `/status disconnected` Sent when the camera goes offline
/// `/status/battery` Sent in reply to a `/query/battery`
///
/// Query Messages:
///
/// `/query/battery` Request that the camera reports its battery level (Not Yet Implemented)
///
///
/// # Usage
///
/// ```bash
/// neolink mqtt --config=config.toml
/// ```
///
/// # Example Config
///
/// ```toml
// [[cameras]]
// name = "Cammy"
// username = "****"
// password = "****"
// address = "****:9000"
//   [cameras.mqtt]
//   server = "127.0.0.1"
//   port = 1883
//   credentials = ["username", "password"]
// ```
//
// `server` is the mqtt server
// `port` is the mqtt server's port
// `credentials` are the username and password required to identify with the mqtt server
//
use std::sync::Arc;

mod app;
mod cmdline;
mod event_cam;
mod mqttc;

use crate::config::{CameraConfig, Config, MqttConfig};
use anyhow::{anyhow, Result};
pub(crate) use app::App;
pub(crate) use cmdline::Opt;
use event_cam::EventCam;
pub(crate) use event_cam::{Direction, Messages};
use mqttc::{Mqtt, MqttReplyRef};

/// Entry point for the mqtt subcommand
///
/// Opt is the command line options
pub(crate) async fn main(_: Opt, config: Config) -> Result<()> {
    let app = App::new();
    let arc_app = Arc::new(app);

    if config.cameras.iter().all(|config| config.mqtt.is_none()) {
        return Err(anyhow!(
            "MQTT command run, but no cameras configured with MQTT settings. Exiting."
        ));
    }

    let mut set = tokio::task::JoinSet::new();
    for camera_config in config
        .cameras
        .iter()
        .map(|a| Arc::new(a.clone()))
        .collect::<Vec<_>>()
    {
        if let Some(mqtt_config) = camera_config.mqtt.as_ref().map(|a| Arc::new(a.clone())) {
            let loop_arc_app = arc_app.clone();
            info!("{}: Setting up mqtt", camera_config.name);
            set.spawn(async move {
                while loop_arc_app.running("app") {
                    let _ =
                        listen_on_camera(camera_config.clone(), &mqtt_config, loop_arc_app.clone())
                            .await;
                }
            });
        }
    }

    if let Some(result) = set.join_next().await {
        result?;
    }

    Ok(())
}

async fn listen_on_camera(
    cam_config: Arc<CameraConfig>,
    mqtt_config: &MqttConfig,
    arc_app: Arc<App>,
) -> Result<()> {
    // Camera thread
    let arc_event_cam = Arc::new(EventCam::new(cam_config.clone(), arc_app.clone()));
    let arc_mqtt = Mqtt::new(mqtt_config, &cam_config.name, arc_app.clone());
    let mut set = tokio::task::JoinSet::new();
    // Start listening to camera events
    let event_cam = arc_event_cam.clone();
    set.spawn(async move {
        event_cam.start_listening().await; // Loop forever
        event_cam.abort(); // Just to ensure everything aborts
    });

    // Start listening to mqtt events
    let event_cam = arc_event_cam.clone();
    let mqtt = arc_mqtt.clone();
    set.spawn(async move {
        let _ = mqtt.start().is_err();
        event_cam.abort();
    });

    // Listen on camera messages and post on mqtt
    let camera_name = cam_config.name.clone();
    let event_cam = arc_event_cam.clone();
    let mqtt = arc_mqtt.clone();
    let app = arc_app.clone();
    set.spawn(async move {
        while app.running(&format!("app: {}", camera_name)) {
            if let Ok(msg) = event_cam.poll().await {
                match msg {
                    Messages::Login => {
                        if mqtt.send_message("status", "connected", true).is_err() {
                            error!("Failed to post connect over MQTT for {}", camera_name);
                        }
                    }
                    Messages::MotionStop => {
                        if mqtt.send_message("status/motion", "off", true).is_err() {
                            error!("Failed to publish motion stop for {}", camera_name);
                        }
                    }
                    Messages::MotionStart => {
                        if mqtt.send_message("status/motion", "on", true).is_err() {
                            error!("Failed to publish motion start for {}", camera_name);
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    // Listen on mqtt messages and post on camera
    let event_cam = arc_event_cam.clone();
    let mqtt = arc_mqtt.clone();
    let app = arc_app.clone();
    let camera_name = cam_config.name.clone();
    set.spawn(async move {
        while app.running(&format!("app: {}", camera_name)) {
            if let Ok(msg) = mqtt.poll() {
                match msg.as_ref() {
                    MqttReplyRef {
                        topic: "control/led",
                        message: "on",
                    } => {
                        if event_cam.send_message(Messages::StatusLedOn).await.is_err() {
                            error!("Failed to set camera status light on");
                        }
                    }
                    MqttReplyRef {
                        topic: "control/led",
                        message: "off",
                    } => {
                        if event_cam
                            .send_message(Messages::StatusLedOff)
                            .await
                            .is_err()
                        {
                            error!("Failed to set camera status light off");
                        }
                    }
                    MqttReplyRef {
                        topic: "control/ir",
                        message: "on",
                    } => {
                        if event_cam.send_message(Messages::IRLedOn).await.is_err() {
                            error!("Failed to set camera status light off");
                        }
                    }
                    MqttReplyRef {
                        topic: "control/ir",
                        message: "off",
                    } => {
                        if event_cam.send_message(Messages::IRLedOff).await.is_err() {
                            error!("Failed to set camera status light off");
                        }
                    }
                    MqttReplyRef {
                        topic: "control/ir",
                        message: "auto",
                    } => {
                        if event_cam.send_message(Messages::IRLedAuto).await.is_err() {
                            error!("Failed to set camera status light off");
                        }
                    }
                    MqttReplyRef {
                        topic: "control/reboot",
                        ..
                    } => {
                        if event_cam.send_message(Messages::Reboot).await.is_err() {
                            error!("Failed to set camera status light off");
                        }
                    }
                    MqttReplyRef {
                        topic: "control/ptz",
                        message,
                    } => {
                        let lowercase_message = message.to_lowercase();
                        let mut words = lowercase_message.split_whitespace();
                        if let Some(direction_txt) = words.next() {
                            let amount = words.next().unwrap_or("32.0");
                            if let Ok(amount) = amount.parse::<f32>() {
                                let direction = match direction_txt {
                                    "up" => Direction::Up(amount),
                                    "down" => Direction::Down(amount),
                                    "left" => Direction::Left(amount),
                                    "right" => Direction::Right(amount),
                                    "in" => Direction::In(amount),
                                    "out" => Direction::Out(amount),
                                    _ => {
                                        error!("Unrecongnized PTZ direction");
                                        continue;
                                    }
                                };
                                if event_cam
                                    .send_message(Messages::Ptz(direction))
                                    .await
                                    .is_err()
                                {
                                    error!("Failed to send PTZ");
                                }
                            } else {
                                error!("No PTZ direction speed was not a valid number");
                            }
                        } else {
                            error!("No PTZ Direction given. Please add up/down/left/right/in/out");
                        }
                    }
                    MqttReplyRef {
                        topic: "query/battery",
                        ..
                    } => match event_cam.send_message_with_reply(Messages::Battery).await {
                        Ok(reply) => {
                            if mqtt.send_message("status/battery", &reply, false).is_err() {
                                error!("Failed to send battery status reply");
                            }
                        }
                        Err(_) => error!("Failed to set camera status light off"),
                    },
                    _ => {}
                }
            }
        }
    });

    while set.join_next().await.is_some() {}

    Ok(())
}

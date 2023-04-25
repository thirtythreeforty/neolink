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
/// - `/control/pir [on|off]` Turns PIR on/off
/// - `/control/ir [on|off|auto]` Turn IR lights on/off or automatically via light detection
/// - `/control/reboot` Reboot the camera
/// - `/control/ptz` [up|down|left|right|in|out] (amount) Control the PTZ movements, amount defaults to 32.0
///
/// Status Messages:
///
/// `/status offline` Sent when the neolink goes offline this is a LastWill message
/// `/status disconnected` Sent when the camera goes offline
/// `/status/battery` Sent in reply to a `/query/battery`
/// `/status/pir` Sent in reply to a `/query/pir`
///
/// Query Messages:
///
/// `/query/battery` Request that the camera reports its battery level
/// `/query/pir` Request that the camera reports its pir status
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
use tokio::time::{sleep, Duration};

mod cmdline;
mod event_cam;
mod mqttc;

use crate::config::{CameraConfig, Config, MqttConfig};
use anyhow::{anyhow, Context, Error, Result};
pub(crate) use cmdline::Opt;
use event_cam::EventCam;
pub(crate) use event_cam::{Direction, Messages};
use log::*;
use mqttc::{Mqtt, MqttReplyRef};

/// Entry point for the mqtt subcommand
///
/// Opt is the command line options
pub(crate) async fn main(_: Opt, config: Config) -> Result<()> {
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
            info!("{}: Setting up mqtt", camera_config.name);
            set.spawn(async move {
                let mut wait_for = Duration::from_micros(125);
                loop {
                    tokio::task::yield_now().await;
                    if let Err(e) = listen_on_camera(camera_config.clone(), &mqtt_config).await {
                        warn!("Error: {:?}. Retrying", e);
                    }
                    sleep(wait_for).await;
                    wait_for *= 2;
                }
            });
        }
    }

    while let Some(result) = set.join_next().await {
        result?;
    }

    Ok(())
}

async fn listen_on_camera(cam_config: Arc<CameraConfig>, mqtt_config: &MqttConfig) -> Result<()> {
    // Camera thread
    let mut event_cam = EventCam::new(cam_config.clone()).await;
    let mut mqtt = Mqtt::new(mqtt_config, &cam_config.name).await;

    let mqtt_sender_cam = mqtt.get_sender();
    let mqtt_sender_mqtt = mqtt.get_sender();
    let event_cam_sender = event_cam.get_sender();

    // Listen on mqtt messages and post on camera
    let camera_name = cam_config.name.clone();

    let mqtt_to_cam = async {
        while let Ok(msg) = mqtt.poll().await {
            tokio::task::yield_now().await;
            let mut reply = None;
            let mut reply_topic = None;
            match msg.as_ref() {
                MqttReplyRef {
                    topic: "control/led",
                    message: "on",
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::StatusLedOn)
                            .await
                            .with_context(|| "Failed to set camera status light on")?,
                    );
                }
                MqttReplyRef {
                    topic: "control/led",
                    message: "off",
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::StatusLedOff)
                            .await
                            .with_context(|| "Failed to set camera status light off")?,
                    );
                }
                MqttReplyRef {
                    topic: "control/ir",
                    message: "on",
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::IRLedOn)
                            .await
                            .with_context(|| "Failed to set camera status light on")?,
                    );
                }
                MqttReplyRef {
                    topic: "control/ir",
                    message: "off",
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::IRLedOff)
                            .await
                            .with_context(|| "Failed to set camera status light off")?,
                    );
                }
                MqttReplyRef {
                    topic: "control/ir",
                    message: "auto",
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::IRLedAuto)
                            .await
                            .with_context(|| "Failed to set camera status light off")?,
                    );
                }
                MqttReplyRef {
                    topic: "control/reboot",
                    ..
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::Reboot)
                            .await
                            .with_context(|| "Failed to set camera status light off")?,
                    );
                }
                MqttReplyRef {
                    topic: "control/ptz",
                    message,
                } => {
                    let lowercase_message = message.to_lowercase();
                    let mut words = lowercase_message.split_whitespace();
                    if let Some(direction_txt) = words.next() {
                        let amount = words.next().unwrap_or("32.0");
                        let seconds = words.next().unwrap_or("1.0");
                        if let Ok(amount) = amount.parse::<f32>() {
                            if let Ok(seconds) = seconds.parse::<f32>() {

                                // range checking on seconds so that you can't sleep for 3.4E+38 seconds
                                match seconds {
                                    x if (0.0..10.0).contains(&x) => seconds,
                                    _ => {
                                        error!("seconds was not a valid number (out of range)");
                                        continue;
                                    }
                                };

                                let direction = match direction_txt {
                                    "up" => Direction::Up(amount, seconds),
                                    "down" => Direction::Down(amount, seconds),
                                    "left" => Direction::Left(amount, seconds),
                                    "right" => Direction::Right(amount, seconds),
                                    "in" => Direction::In(amount, seconds),
                                    "out" => Direction::Out(amount, seconds),
                                    _ => {
                                        error!("Unrecognized PTZ direction \"{}\"", direction_txt);
                                        continue;
                                    }
                                };
                                reply = Some(
                                    event_cam_sender
                                        .send_message_with_reply(Messages::Ptz(direction))
                                        .await
                                        .with_context(|| "Failed to send PTZ")?,
                                );
                            } else {
                                error!("seconds was not a valid number (unrecognized)");
                            }
                        } else {
                            error!("No PTZ direction speed was not a valid number");
                        }
                    } else {
                        error!("No PTZ Direction given. Please add up/down/left/right/in/out");
                    }
                }
                MqttReplyRef {
                    topic: "control/pir",
                    message: "on",
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::PIROn)
                            .await
                            .with_context(|| "Failed to set pir on")?,
                    );
                }
                MqttReplyRef {
                    topic: "control/pir",
                    message: "off",
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::PIROff)
                            .await
                            .with_context(|| "Failed to set pir off")?,
                    );
                }
                MqttReplyRef {
                    topic: "query/battery",
                    ..
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::Battery)
                            .await
                            .with_context(|| "Failed to get battery status")?,
                    );
                    reply_topic = Some("status/battery");
                }
                MqttReplyRef {
                    topic: "query/pir", ..
                } => {
                    reply = Some(
                        event_cam_sender
                            .send_message_with_reply(Messages::PIRQuery)
                            .await
                            .with_context(|| "Failed to get pir status")?,
                    );
                    reply_topic = Some("status/pir");
                }
                _ => {}
            }
            if let Some(reply) = reply {
                if let Some(topic) = reply_topic {
                    mqtt_sender_mqtt
                        .send_message(topic, &reply, false)
                        .await
                        .with_context(|| "Failed to send Camera reply to Mqtt")?;
                } else {
                    mqtt_sender_mqtt
                        .send_message(&msg.topic, &reply, false)
                        .await
                        .with_context(|| "Failed to send Camera reply to Mqtt")?;
                }
            }
        }
        Result::<(), Error>::Ok(())
    };

    let cam_to_mqtt = async {
        loop {
            tokio::task::yield_now().await;
            match event_cam.poll().await? {
                Messages::Login => {
                    mqtt_sender_cam
                        .send_message("status", "connected", true)
                        .await
                        .with_context(|| {
                            format!("Failed to post connect over MQTT for {}", camera_name)
                        })?;
                }
                Messages::MotionStop => {
                    mqtt_sender_cam
                        .send_message("status/motion", "off", true)
                        .await
                        .with_context(|| {
                            format!("Failed to publish motion stop for {}", camera_name)
                        })?;
                }
                Messages::MotionStart => {
                    mqtt_sender_cam
                        .send_message("status/motion", "on", true)
                        .await
                        .with_context(|| {
                            format!("Failed to publish motion start for {}", camera_name)
                        })?;
                }
                _ => {}
            }
        }
    };

    tokio::select! {
        v = mqtt_to_cam => {v},
        v = cam_to_mqtt => {v},
    }?;

    Ok(())
}

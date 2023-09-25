//!
//! # Neolink MQTT
//!
//! Handles incoming and outgoing MQTT messages
//!
//! This acts as a bridge between cameras and MQTT servers
//!
//! Messages are prefixed with `neolink/{CAMERANAME}`
//!
//! Control messages:
//!
//! - `/control/floodlight [on|off]` Turns floodlight (if equipped) on/off
//! - `/control/led [on|off]` Turns status LED on/off
//! - `/control/pir [on|off]` Turns PIR on/off
//! - `/control/ir [on|off|auto]` Turn IR lights on/off or automatically via light detection
//! - `/control/reboot` Reboot the camera
//! - `/control/ptz` [up|down|left|right|in|out] (amount) Control the PTZ movements, amount defaults to 32.0
//! - `/control/ptz/preset` [id] Move the camera to a known preset
//! - `/control/ptz/assign` [id] [name] Assign the current ptz position to an ID and name
//!
//! Status Messages:
//!
//! `/status offline` Sent when the neolink goes offline this is a LastWill message
//! `/status disconnected` Sent when the camera goes offline
//! `/status/battery` Sent in reply to a `/query/battery`
//! `/status/pir` Sent in reply to a `/query/pir`
//! `/status/ptz/preset` Sent in reply to a `/query/ptz/preset`
//!
//! Query Messages:
//!
//! `/query/battery` Request that the camera reports its battery level
//! `/query/pir` Request that the camera reports its pir status
//! `/query/ptz/preset` Request that the camera reports the PTZ presets
//! `/query/preview` Request that the camera post a base64 encoded jpeg
//!    of the stream to `/status/preview`
//!
//!
//! # Usage
//!
//! ```bash
//! neolink mqtt --config=config.toml
//! ```
//!
//! # Example Config
//!
//! ```toml
//! [[cameras]]
//! name = "Cammy"
//! username = "****"
//! password = "****"
//! address = "****:9000"
//!   [cameras.mqtt]
//!   server = "127.0.0.1"
//!   port = 1883
//!   credentials = ["username", "password"]
//! ```
//!
//! `server` is the mqtt server
//! `port` is the mqtt server's port
//! `credentials` are the username and password required to identify with the mqtt server
//!
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::collections::{HashMap, HashSet};
use tokio::{
    sync::mpsc::channel as mpsc,
    task::JoinSet,
    time::{interval, sleep, Duration, MissedTickBehavior},
};
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use tokio_util::sync::CancellationToken;
use validator::Validate;

use neolink_core::bc_protocol::{Direction as BcDirection, LightState};

mod cmdline;
mod discovery;
mod mqttc;

use crate::{
    common::{MdState, NeoInstance, NeoReactor},
    config::Config,
    AnyResult,
};
use anyhow::{anyhow, Context, Result};
pub(crate) use cmdline::Opt;
pub(crate) use discovery::Discoveries;
use log::*;
use mqttc::{Mqtt, MqttReplyRef};

use self::{
    discovery::enable_discovery,
    mqttc::{MqttInstance, MqttReply},
};

/// Entry point for the mqtt subcommand
///
/// Opt is the command line options
pub(crate) async fn main(_: Opt, reactor: NeoReactor) -> Result<()> {
    let mut set = tokio::task::JoinSet::new();
    let global_cancel = CancellationToken::new();
    let cancel_drop = global_cancel.clone().drop_guard();
    let config = reactor.config().await?;
    let mqtt = Mqtt::new(config.clone()).await;

    // Startup and stop cameras as they are added/removed to the config
    let thread_cancel = global_cancel.clone();
    let mut thread_config = config.clone();
    let thread_reactor = reactor.clone();
    let thread_instance = mqtt.subscribe("").await?;
    set.spawn(async move {
        let mut set = JoinSet::<AnyResult<()>>::new();
        let thread_cancel2 = thread_cancel.clone();
        tokio::select!{
            _ = thread_cancel.cancelled() => AnyResult::Ok(()),
            v = async {
                let mut cameras: HashMap<String, CancellationToken> = Default::default();
                let mut config_names = HashSet::new();
                loop {
                    thread_config.wait_for(|config| {
                        let current_names = config.cameras.iter().filter(|a| a.enabled).map(|cam_config| cam_config.name.clone()).collect::<HashSet<_>>();
                        current_names != config_names
                    }).await.with_context(|| "Camera Config Watcher")?;
                    config_names = thread_config.borrow().clone().cameras.iter().filter(|a| a.enabled).map(|cam_config| cam_config.name.clone()).collect::<HashSet<_>>();

                    for name in config_names.iter() {
                        log::info!("{name}: MQTT Staring");
                        if ! cameras.contains_key(name) {
                            let local_cancel = CancellationToken::new();
                            cameras.insert(name.clone(),local_cancel.clone());

                            let thread_global_cancel = thread_cancel2.clone();
                            let thread_reactor2 = thread_reactor.clone();
                            let mqtt_instance = thread_instance.subscribe(name).await?;
                            let name = name.clone();
                            set.spawn(async move {
                                loop {
                                    let camera = thread_reactor2.get(&name).await?;
                                    let mqtt_instance = mqtt_instance.resubscribe().await?;
                                    let r = tokio::select!{
                                        _ = thread_global_cancel.cancelled() => {
                                            AnyResult::Ok(())
                                        },
                                        _ = local_cancel.cancelled() => {
                                            AnyResult::Ok(())
                                        },
                                        v = listen_on_camera(camera, mqtt_instance) => {
                                            v
                                        },
                                    };
                                    if let Ok(()) = &r {
                                        break r
                                    } else {
                                        log::debug!("listen_on_camera stopped: {:?}", r);
                                        continue;
                                    }
                                }
                            }) ;
                        }
                    }

                    for (running_name, token) in cameras.iter() {
                        if ! config_names.contains(running_name) {
                            log::debug!("Mqtt::main Cancel");
                            token.cancel();
                        }
                    }
                }
            } => v,
        }
    });

    // This threads prints the config
    let mut thread_config = config.clone();
    let thread_instance = mqtt.subscribe("").await?;
    let thread_cancel = global_cancel.clone();
    set.spawn(async move {
        tokio::select! {
            _ = thread_cancel.cancelled() => AnyResult::Ok(()),
            v = async {
                let mut curr_config = thread_config.borrow().clone();
                let str = toml::to_string(&curr_config)?;
                thread_instance.send_message("config", &str, true).await?;
                loop {
                    curr_config = thread_config
                        .wait_for(|new_conf| new_conf != &curr_config)
                        .await?
                        .clone();
                    let str = toml::to_string(&curr_config)?;
                    thread_instance.send_message("config", &str, true).await?;
                    log::trace!("UpdatedPosted config");
                }
            } => v,
        }
    });

    // This threads checks for config changes on the mqtt
    let thread_config = config.clone();
    let mut thread_instance = mqtt.subscribe("").await?;
    let thread_reactor = reactor.clone();
    let thread_cancel = global_cancel.clone();
    set.spawn(async move {
        tokio::select! {
            _ = thread_cancel.cancelled() => AnyResult::Ok(()),
            v = async {
                while let Ok(msg) = thread_instance.recv().await {
                    if msg.topic == "config" {
                        let config: Result<Config> = toml::from_str(&msg.message).with_context(|| {
                            format!("Failed to parse the MQTT {:?} config file", msg.topic)
                        });
                        if let Err(e) = config {
                            thread_instance
                                .send_message("config/status", &format!("{:?}", e), false)
                                .await?;
                            continue;
                        }
                        let config = config?;

                        let validate = config.validate().with_context(|| {
                            format!("Failed to validate the MQTT {:?} config file", msg.topic)
                        });
                        if let Err(e) = validate {
                            thread_instance
                                .send_message("config/status", &format!("{:?}", e), false)
                                .await?;
                            continue;
                        }

                        if (*thread_config.borrow()) == config {
                            continue;
                        }

                        let result = thread_reactor.update_config(config).await;
                        thread_instance
                            .send_message("config/status", &format!("{:?}", result), false)
                            .await?;
                        log::info!("Updated config");
                    }
                }
                AnyResult::Ok(())
            } => v,
        }
    });

    while let Some(result) = set.join_next().await {
        if let Err(_) | Ok(Err(_)) = &result {
            global_cancel.cancel();
            result??;
        }
    }

    drop(cancel_drop);
    Ok(())
}

async fn listen_on_camera(camera: NeoInstance, mqtt_instance: MqttInstance) -> Result<()> {
    let mut watch_config = camera.config().await?;
    let camera_name = watch_config.borrow().name.clone();
    let mut config;
    let cancel = CancellationToken::new();
    let drop_cancel = cancel.clone().drop_guard();
    loop {
        config = watch_config.borrow().clone().mqtt;
        break tokio::select! {
            v = watch_config.wait_for(|new_config| config != new_config.mqtt) => {
                v?;
                continue;
            }
            v = async {
                //Publish initial states
                mqtt_instance
                        .send_message("status", "disconnected", true)
                        .await
                        .with_context(|| format!("Failed to publish status for {}", camera_name))?;
                let _drop_message = mqtt_instance.drop_guard_message("status", "disconnected").await?;
                mqtt_instance
                    .send_message("status/motion", "unknown", true)
                    .await
                    .with_context(|| format!("Failed to publish motion unknown for {}", camera_name))?;
                let _drop_message2 = mqtt_instance.drop_guard_message("status/motion", "unknown").await?;

                if let Some(discovery_config) = config.discovery.as_ref() {
                    enable_discovery(discovery_config, &mqtt_instance, &camera).await?;
                }

                let camera_msg = camera.clone();
                let mut mqtt_msg = mqtt_instance.resubscribe().await?;
                let cancel_msg = cancel.clone();
                let mut set_msg = JoinSet::new();

                let mut camera_watch = camera.camera();
                let mqtt_watch = mqtt_instance.resubscribe().await?;

                let camera_floodlight = camera.clone();
                let mqtt_floodlight = mqtt_instance.resubscribe().await?;

                let camera_motion = camera.clone();
                let mqtt_motion = mqtt_instance.resubscribe().await?;

                let camera_snap = camera.clone();
                let mqtt_snap = mqtt_instance.resubscribe().await?;

                let camera_battery = camera.clone();
                let mqtt_battery = mqtt_instance.resubscribe().await?;

                tokio::select! {
                    _ = cancel.cancelled() => AnyResult::Ok(()),
                    // Handles incomming requests
                    v  = async {
                        let (tx, mut rx) = mpsc(1);
                        tokio::select! {
                            v = async {
                                log::debug!("Listening to message on {}", mqtt_msg.get_name());
                                while let Ok(msg) = mqtt_msg.recv().await {
                                    let mqtt_msg = mqtt_msg.resubscribe().await?;
                                    let camera_msg = camera_msg.clone();
                                    let tx = tx.clone();
                                    let cancel_msg = cancel_msg.clone();
                                    set_msg.spawn(async move {
                                        tokio::select!{
                                            _ = cancel_msg.cancelled() => AnyResult::Ok(()),
                                            v = async {
                                                // log::debug!("Got message: {msg:?}");
                                                let res = handle_mqtt_message(msg, &mqtt_msg, &camera_msg).await;
                                                if res.is_err() {
                                                    tx.send(res).await?;
                                                }
                                                AnyResult::Ok(())
                                            } => v,
                                        }
                                    });
                                }
                                log::debug!("Listening to message on {}", mqtt_msg.get_name());
                                AnyResult::Ok(())
                            } => v,
                            v = rx.recv() => {
                                v.ok_or(anyhow!("All error senders were dropped"))?
                            },
                        }?;
                        AnyResult::Ok(())
                    } => v,
                    // Handle camera disconnect/connect
                    v = async {
                        loop {
                            camera_watch.wait_for(|cam| cam.upgrade().is_some()).await.with_context(|| {
                                format!("{}: Online Watch Dropped", camera_name)
                            })?;
                            log::trace!("Publish online");
                            mqtt_watch.send_message("status", "connected", true).await.with_context(|| {
                                format!("{}: Failed to publish connected", camera_name)
                            })?;
                            camera_watch.wait_for(|cam| cam.upgrade().is_none()).await.with_context(|| {
                                format!("{}: Disconnect Watch Dropped", camera_name)
                            })?;
                            mqtt_watch.send_message("status", "disconnected", true).await.with_context(|| {
                                format!("{}: Failed to publish disconnected", camera_name)
                            })?;
                        }
                    } => v,
                    // Handle the floodlight
                    v = async {
                        let (tx, mut rx) = mpsc(100);
                        let v = tokio::select! {
                            v = camera_floodlight.run_passive_task(|cam| {
                                let tx = tx.clone();
                                Box::pin(
                                    async move {
                                        let mut reciever = tokio_stream::wrappers::ReceiverStream::new(cam.listen_on_flightlight().await?);
                                        while let Some(flights) = reciever.next().await {
                                            for flight in flights.floodlight_status_list.iter() {
                                                if flight.status == 0 {
                                                    tx.send(false).await?;
                                                } else {
                                                    tx.send(true).await?;
                                                }
                                            }
                                        }
                                        AnyResult::Ok(())
                                    }
                                )
                            }) => v,
                            v = async {
                                while let Some(on) = rx.recv().await {
                                    if on {
                                        mqtt_floodlight.send_message("status/floodlight", "on", true).await?;
                                    } else {
                                        mqtt_floodlight.send_message("status/floodlight", "off", true).await?;
                                    }
                                }
                                AnyResult::Ok(())
                            } => v,
                        };
                        match v.map_err(|e| e.downcast::<neolink_core::Error>()) {
                            Err(Ok(neolink_core::Error::UnintelligibleReply{..})) => futures::future::pending().await,
                            Ok(()) => AnyResult::Ok(()),
                            Err(Ok(e)) => Err(e.into()),
                            Err(Err(e)) => Err(e),
                        }?;
                        AnyResult::Ok(())
                    } => v,
                    // Handle the motion messages
                    v = async {
                        let mut md = camera_motion.motion().await?;
                        loop {
                            let v = async {
                                md.wait_for(|state| matches!(state, MdState::Start(_))).await.with_context(|| {
                                    format!("{}: MdStart Watch Dropped", camera_name)
                                })?;
                                mqtt_motion.send_message("status/motion", "on", true).await.with_context(|| {
                                    format!("{}: Failed to publish motion start", camera_name)
                                })?;
                                md.wait_for(|state| matches!(state, MdState::Stop(_))).await.with_context(|| {
                                    format!("{}: MdStop Watch Dropped", camera_name)
                                })?;
                                mqtt_motion.send_message("status/motion", "off", true).await.with_context(|| {
                                    format!("{}: Failed to publish motion stop", camera_name)
                                })?;
                                AnyResult::Ok(())
                            }.await;
                            match v.map_err(|e| e.downcast::<neolink_core::Error>()) {
                                Err(Ok(neolink_core::Error::UnintelligibleReply{..})) => futures::future::pending().await,
                                Ok(()) => AnyResult::Ok(()),
                                Err(Ok(e)) => Err(e.into()),
                                Err(Err(e)) => Err(e),
                            }?;
                        }
                    } => v,
                    // Handle the SNAP (image preview)
                    v = async {
                        let mut wait = IntervalStream::new({
                            let mut i = interval(Duration::from_millis(config.preview_update));
                            i.set_missed_tick_behavior(MissedTickBehavior::Skip);
                            i
                        });
                        let v = async {
                            while wait.next().await.is_some() {
                                let image = camera_snap.run_passive_task(|cam| {
                                    Box::pin(async move {
                                        let image = cam.get_snapshot().await?;
                                        AnyResult::Ok(image)
                                    })
                                }).await;
                                let image = match image {
                                    Err(e) => match e.downcast::<neolink_core::Error>() {
                                        Ok(neolink_core::Error::CameraServiceUnavaliable) => {
                                            log::debug!("Image not supported");
                                            futures::future::pending().await
                                        },
                                        Ok(e) => Err(e.into()),
                                        Err(e) => Err(e),
                                    }
                                    n => n,
                                }?;
                                mqtt_snap
                                        .send_message("status/preview", BASE64.encode(image).as_str(), true)
                                        .await
                                        .with_context(|| {
                                            format!("{}: Failed to publish preview", camera_name)
                                        })?;
                            }
                            AnyResult::Ok(())
                        }.await;
                        match v.map_err(|e| e.downcast::<neolink_core::Error>()) {
                            Err(Ok(neolink_core::Error::UnintelligibleReply{..})) => futures::future::pending().await,
                            Ok(()) => AnyResult::Ok(()),
                            Err(Ok(e)) => Err(e.into()),
                            Err(Err(e)) => Err(e),
                        }?;
                        AnyResult::Ok(())
                    } => v,
                    // Handle the battery publish
                    v = async {
                        let mut wait = IntervalStream::new({
                            let mut i = interval(Duration::from_millis(config.battery_update));
                            i.set_missed_tick_behavior(MissedTickBehavior::Skip);
                            i
                        });

                        let v = async {
                            while wait.next().await.is_some() {
                                let xml = camera_battery.run_passive_task(|cam| {
                                    Box::pin(async move {
                                        let xml = cam.battery_info().await?;
                                        AnyResult::Ok(xml)
                                    })
                                }).await;
                                let xml = match xml {
                                    Err(e) => match e.downcast::<neolink_core::Error>() {
                                        Ok(neolink_core::Error::CameraServiceUnavaliable) => {
                                            log::debug!("Battery not supported");
                                            futures::future::pending().await
                                        },
                                        Ok(e) => Err(e.into()),
                                        Err(e) => Err(e),
                                    }
                                    n => n,
                                }?;
                                mqtt_battery
                                        .send_message("status/battery_level", format!("{}", xml.battery_percent).as_str(), true)
                                        .await
                                        .with_context(|| {
                                            format!("{}: Failed to publish battery", camera_name)
                                        })?;
                            }
                            AnyResult::Ok(())
                        }.await;
                        match v.map_err(|e| e.downcast::<neolink_core::Error>()) {
                            Err(Ok(neolink_core::Error::UnintelligibleReply{..})) => futures::future::pending().await,
                            Ok(()) => AnyResult::Ok(()),
                            Err(Ok(e)) => Err(e.into()),
                            Err(Err(e)) => Err(e),
                        }?;
                        AnyResult::Ok(())
                    } => v
                }?;
                AnyResult::Ok(())
            } => v,
        };
    }?;

    log::debug!("Mqtt::listen_on_camera Cancel");
    drop(drop_cancel);
    Ok(())
}

async fn handle_mqtt_message(
    msg: MqttReply,
    mqtt: &MqttInstance,
    camera: &NeoInstance,
) -> Result<()> {
    match msg.as_ref() {
        MqttReplyRef {
            topic: _,
            message: "OK",
        }
        | MqttReplyRef {
            topic: _,
            message: "FAIL",
        } => {
            // Do nothing for the success/fail replies
        }
        MqttReplyRef {
            topic: "control/floodlight",
            message: "on",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.set_floodlight_manual(true, 180).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn on the floodlight light: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/floodlight", &reply, false)
                .await
                .with_context(|| "Failed to publish camera status light on")?;
        }
        MqttReplyRef {
            topic: "control/floodlight",
            message: "off",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.set_floodlight_manual(false, 180).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn off the floodlight light: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/floodlight", &reply, false)
                .await
                .with_context(|| "Failed to publish camera status light off")?;
        }
        MqttReplyRef {
            topic: "control/led",
            message: "on",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.led_light_set(true).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn on the led: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/led", &reply, false)
                .await
                .with_context(|| "Failed to publish led on")?;
        }
        MqttReplyRef {
            topic: "control/led",
            message: "off",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.led_light_set(false).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn off the led: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/led", &reply, false)
                .await
                .with_context(|| "Failed to publish led off")?;
        }
        MqttReplyRef {
            topic: "control/ir",
            message: "on",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.irled_light_set(LightState::On).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn on the ir: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/ir", &reply, false)
                .await
                .with_context(|| "Failed to publish ir on")?;
        }
        MqttReplyRef {
            topic: "control/ir",
            message: "off",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.irled_light_set(LightState::Off).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn off the ir: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/ir", &reply, false)
                .await
                .with_context(|| "Failed to publish ir off")?;
        }
        MqttReplyRef {
            topic: "control/ir",
            message: "auto",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.irled_light_set(LightState::Auto).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn set to auto on the led: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/ir", &reply, false)
                .await
                .with_context(|| "Failed to publish ir auto")?;
        }
        MqttReplyRef {
            topic: "control/reboot",
            ..
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.reboot().await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to reboot the camera: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/ir", &reply, false)
                .await
                .with_context(|| "Failed to publish reboot on the camera")?;
        }
        MqttReplyRef {
            topic: "control/ptz",
            message,
        } => {
            let lowercase_message = message.to_lowercase();
            let mut words = lowercase_message.split_whitespace();
            let reply = if let Some(direction_txt) = words.next() {
                // Target amount to move
                let speed = 32f32;
                let amount = words.next().unwrap_or("32.0");

                if let Ok(amount) = amount.parse::<f32>() {
                    let seconds = amount / speed;
                    // range checking on seconds so that you can't sleep for 3.4E+38 seconds
                    let seconds = match seconds {
                        x if (0.0..10.0).contains(&x) => Some(seconds),
                        _ => {
                            error!("seconds was not a valid number (out of range)");
                            None
                        }
                    };

                    let bc_direction = match direction_txt {
                        "up" => Some(BcDirection::Up),
                        "down" => Some(BcDirection::Down),
                        "left" => Some(BcDirection::Left),
                        "right" => Some(BcDirection::Right),
                        "in" => Some(BcDirection::In),
                        "out" => Some(BcDirection::Out),
                        n => {
                            error!("Unrecognized PTZ direction \"{}\"", n);
                            None
                        }
                    };

                    if let (Some(seconds), Some(bc_direction)) = (seconds, bc_direction) {
                        // On drop send the stop command again just to make sure it stops
                        let _drop_command = camera.clone().drop_command(
                            move |cam| {
                                Box::pin(async move {
                                    cam.send_ptz(BcDirection::Stop, speed).await?;
                                    AnyResult::Ok(())
                                })
                            },
                            Duration::from_millis(100),
                        );
                        if let Err(e) = camera
                            .run_task(|cam| {
                                Box::pin(async move {
                                    cam.send_ptz(bc_direction, speed).await?;
                                    sleep(Duration::from_secs_f32(seconds)).await;
                                    cam.send_ptz(BcDirection::Stop, speed).await?;
                                    AnyResult::Ok(())
                                })
                            })
                            .await
                        {
                            error!("Failed to send PTZ: {:?}", e);
                            "FAIL"
                        } else {
                            "OK"
                        }
                    } else {
                        "FAIL"
                    }
                } else {
                    error!("No PTZ speed as a valid number");
                    "FAIL"
                }
            } else {
                error!("No PTZ Direction given. Please add up/down/left/right/in/out");
                "FAIL"
            }
            .to_string();

            mqtt.send_message("control/ptz", &reply, false)
                .await
                .with_context(|| "Failed to publish reboot on the camera")?;
        }
        MqttReplyRef {
            topic: "control/ptz/preset",
            message,
        } => {
            let reply = if let Ok(id) = message.parse::<u8>() {
                let res = camera
                    .run_task(|cam| {
                        Box::pin(async move {
                            cam.moveto_ptz_preset(id).await?;
                            AnyResult::Ok(())
                        })
                    })
                    .await;
                if res.is_err() {
                    error!("Failed to move to ptz preset: {:?}", res.err());
                    "FAIL"
                } else {
                    "OK"
                }
            } else {
                error!("PTZ preset was not a valid number");
                "FAIL"
            }
            .to_string();
            mqtt.send_message("control/ir", &reply, false)
                .await
                .with_context(|| "Failed to publish ptz move")?;
        }
        MqttReplyRef {
            topic: "control/ptz/assign",
            message,
        } => {
            let mut words = message.split_whitespace();
            let id = words.next();
            let name = words.next();

            let reply = if let (Some(Ok(id)), Some(name)) = (id.map(|id| id.parse::<u8>()), name) {
                let name = name.to_owned();
                let res = camera
                    .run_task(|cam| {
                        let name = name.clone();
                        Box::pin(async move {
                            cam.set_ptz_preset(id, name).await?;
                            AnyResult::Ok(())
                        })
                    })
                    .await;
                if res.is_err() {
                    error!("Failed to assign ptz preset: {:?}", res.err());
                    "FAIL"
                } else {
                    "OK"
                }
            } else if let (Some(Err(_)), _) = (id.map(|id| id.parse::<u8>()), name) {
                error!("PTZ preset was not a valid number");
                "FAIL"
            } else if let (_, None) = (id.map(|id| id.parse::<u8>()), name) {
                error!("PTZ preset was not given a name");
                "FAIL"
            } else {
                "FAIL"
            }
            .to_string();
            mqtt.send_message("control/ir", &reply, false)
                .await
                .with_context(|| "Failed to publish ptz move")?;
        }
        MqttReplyRef {
            topic: "control/pir",
            message: "on",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.pir_set(true).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn on the pir: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/pir", &reply, false)
                .await
                .with_context(|| "Failed to publish pir on")?;
        }
        MqttReplyRef {
            topic: "control/pir",
            message: "off",
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        cam.pir_set(false).await?;
                        AnyResult::Ok(())
                    })
                })
                .await;
            let reply = if res.is_err() {
                error!("Failed to turn off the pir: {:?}", res.err());
                "FAIL"
            } else {
                "OK"
            }
            .to_string();
            mqtt.send_message("control/pir", &reply, false)
                .await
                .with_context(|| "Failed to publish pir off")?;
        }
        MqttReplyRef {
            topic: "query/battery",
            ..
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        let xml = cam.battery_info().await?;
                        AnyResult::Ok(xml)
                    })
                })
                .await;
            let reply = match res {
                Err(e) => {
                    error!("Failed to get battery xml: {:?}", e);
                    "FAIL"
                }
                Ok(xml) => {
                    let bytes_res =
                        yaserde::ser::serialize_with_writer(&xml, vec![], &Default::default());
                    match bytes_res {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(str) => {
                                mqtt.send_message("status/battery", &str, false)
                                    .await
                                    .with_context(|| "Failed to publish battery info")?;
                                "OK"
                            }
                            Err(_) => {
                                error!("Failed to encode battery status");
                                "FAIL"
                            }
                        },
                        Err(_) => {
                            error!("Failed to serialise battery status");
                            "FAIL"
                        }
                    }
                }
            }
            .to_string();
            mqtt.send_message("query/battery", &reply, false)
                .await
                .with_context(|| "Failed to publish battery query")?;
        }
        MqttReplyRef {
            topic: "query/pir", ..
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        let xml = cam.get_pirstate().await?;
                        AnyResult::Ok(xml)
                    })
                })
                .await;
            let reply = match res {
                Err(e) => {
                    error!("Failed to get pir xml: {:?}", e);
                    "FAIL"
                }
                Ok(xml) => {
                    let bytes_res =
                        yaserde::ser::serialize_with_writer(&xml, vec![], &Default::default());
                    match bytes_res {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(str) => {
                                mqtt.send_message("status/pir", &str, false)
                                    .await
                                    .with_context(|| "Failed to publish pir info")?;
                                "OK"
                            }
                            Err(_) => {
                                error!("Failed to encode pir status");
                                "FAIL"
                            }
                        },
                        Err(_) => {
                            error!("Failed to serialise pir status");
                            "FAIL"
                        }
                    }
                }
            }
            .to_string();
            mqtt.send_message("query/pir", &reply, false)
                .await
                .with_context(|| "Failed to publish pir query")?;
        }
        MqttReplyRef {
            topic: "query/ptz/preset",
            ..
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        let xml = cam.get_ptz_preset().await?;
                        AnyResult::Ok(xml)
                    })
                })
                .await;
            let reply = match res {
                Err(e) => {
                    error!("Failed to get ptz xml: {:?}", e);
                    "FAIL"
                }
                Ok(xml) => {
                    let bytes_res =
                        yaserde::ser::serialize_with_writer(&xml, vec![], &Default::default());
                    match bytes_res {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(str) => {
                                mqtt.send_message("status/ptz", &str, false)
                                    .await
                                    .with_context(|| "Failed to publish ptz info")?;
                                "OK"
                            }
                            Err(_) => {
                                error!("Failed to encode ptz status");
                                "FAIL"
                            }
                        },
                        Err(_) => {
                            error!("Failed to serialise ptz status");
                            "FAIL"
                        }
                    }
                }
            }
            .to_string();
            mqtt.send_message("query/ptz", &reply, false)
                .await
                .with_context(|| "Failed to publish ptz query")?;
        }
        MqttReplyRef {
            topic: "query/preview",
            ..
        } => {
            let res = camera
                .run_task(|cam| {
                    Box::pin(async move {
                        let data = cam.get_snapshot().await?;
                        AnyResult::Ok(data)
                    })
                })
                .await;
            let reply = match res {
                Err(e) => {
                    error!("Failed to get snapshot: {:?}", e);
                    "FAIL"
                }
                Ok(bytes) => {
                    if let Err(e) = mqtt
                        .send_message("status/preview", BASE64.encode(bytes).as_str(), true)
                        .await
                        .with_context(|| "Failed to publish preview")
                    {
                        error!("Failed to send preview: {e:?}");
                        "FAIL"
                    } else {
                        "OK"
                    }
                }
            }
            .to_string();
            mqtt.send_message("query/preview", &reply, false)
                .await
                .with_context(|| "Failed to publish preview query")?;
        }
        _ => {}
    }
    Ok(())
}

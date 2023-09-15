//! Enable and configures home assistant MQTTT discovery
//!
//! https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery
//!
use anyhow::{Context, Result};
use heck::ToTitleCase;
use log::*;

use super::mqttc::MqttInstance;
use crate::{common::NeoInstance, config::MqttDiscoveryConfig};
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Copy, Hash)]
pub(crate) enum Discoveries {
    #[serde(alias = "floodlight", alias = "light")]
    Floodlight,
    #[serde(alias = "camera")]
    Camera,
    #[serde(alias = "motion", alias = "md", alias = "pir")]
    Motion,
    #[serde(alias = "led")]
    Led,
    #[serde(alias = "ir")]
    Ir,
    #[serde(alias = "reboot")]
    Reboot,
    #[serde(alias = "pt")]
    Pt,
    #[serde(alias = "battery", alias = "power")]
    Battery,
}

#[derive(Debug, Clone)]
struct DiscoveryConnection {
    connection_type: String,
    connection_id: String,
}

impl Serialize for DiscoveryConnection {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        vec![&self.connection_type, &self.connection_id].serialize(serializer)
    }
}

#[derive(Serialize, Debug, Clone)]
struct DiscoveryDevice {
    name: String,
    connections: Vec<DiscoveryConnection>,
    identifiers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sw_version: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
struct DiscoveryAvaliablity {
    topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_available: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_not_available: Option<String>,
}

#[derive(Serialize, Debug)]
struct DiscoveryLight {
    name: String,
    unique_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
    // Light specific
    #[serde(skip_serializing_if = "Option::is_none")]
    state_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_value_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_topic: Option<String>,
    payload_on: String,
    payload_off: String,
}

#[derive(Serialize, Debug)]
#[allow(dead_code)]
enum Encoding {
    None,
    #[serde(rename = "b64")]
    Base64,
}

impl Encoding {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Serialize, Debug)]
struct DiscoveryCamera {
    name: String,
    unique_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
    // Camera specific
    topic: String,
    #[serde(skip_serializing_if = "Encoding::is_none")]
    image_encoding: Encoding,
}

#[derive(Serialize, Debug)]
struct DiscoverySwitch {
    name: String,
    unique_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
    // Switch specific
    // - Control
    command_topic: String,
    payload_off: String,
    payload_on: String,
    // - State
    #[serde(skip_serializing_if = "Option::is_none")]
    state_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_off: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_on: Option<String>,
}

#[derive(Serialize, Debug)]
struct DiscoverySelect {
    name: String,
    unique_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
    // Switch specific
    // - Control
    command_topic: String,
    options: Vec<String>,
    // - State
    #[serde(skip_serializing_if = "Option::is_none")]
    state_topic: Option<String>,
}

#[derive(Serialize, Debug)]
struct DiscoveryBinarySensor {
    name: String,
    unique_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
    // BinarySensor specific
    payload_off: String,
    payload_on: String,
    // - State
    state_topic: String,
}

#[derive(Serialize, Debug)]
struct DiscoveryButton {
    name: String,
    unique_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
    // Button specific
    command_topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_press: Option<String>,
}

#[derive(Serialize, Debug)]
struct DiscoverySensor {
    name: String,
    unique_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
    // Button specific
    state_topic: String,
    state_class: String,
    unit_of_measurement: String,
}

/// Enables MQTT discovery for a camera. See docs at https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery
pub(crate) async fn enable_discovery(
    discovery_config: &MqttDiscoveryConfig,
    mqtt: &MqttInstance,
    camera: &NeoInstance,
) -> Result<()> {
    let cam_config = camera.config().await?.borrow().clone();
    debug!("Enabling MQTT discovery for {}", cam_config.name);

    let mut connections = vec![];
    if let Some(addr) = &cam_config.camera_addr {
        connections.push(DiscoveryConnection {
            connection_type: "camera_addr".to_string(),
            connection_id: addr.clone(),
        });
    }
    if let Some(uid) = &cam_config.camera_uid {
        connections.push(DiscoveryConnection {
            connection_type: "camera_uid".to_string(),
            connection_id: uid.clone(),
        });
    }

    if connections.is_empty() {
        error!(
            "No connections found for camera {}, either addr or UID must be supplied",
            cam_config.name
        );
        return Ok(());
    }

    let friendly_name = cam_config.name.replace('_', " ").to_title_case();
    let device = DiscoveryDevice {
        name: friendly_name.clone(),
        connections,
        identifiers: vec![format!("neolink_{}", cam_config.name)],
        manufacturer: Some("Reolink".to_string()),
        model: Some("Neolink".to_string()),
        sw_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    let availability = DiscoveryAvaliablity {
        topic: format!("neolink/{}/status", cam_config.name),
        payload_available: Some("connected".to_string()),
        payload_not_available: None,
    };

    for feature in &discovery_config.features {
        match feature {
            Discoveries::Floodlight => {
                let config_data = DiscoveryLight {
                    // Common across all potential features
                    device: device.clone(),
                    availability: availability.clone(),

                    // Identifiers
                    name: format!("{} Floodlight", friendly_name.as_str()),
                    unique_id: format!("neolink_{}_floodlight", cam_config.name),
                    // Match native home assistant integration: https://github.com/home-assistant/core/blob/dev/homeassistant/components/reolink/light.py#L49
                    icon: Some("mdi:spotlight-beam".to_string()),

                    // State
                    state_topic: Some(format!("neolink/{}/status/floodlight", cam_config.name)),
                    state_value_template: Some("{{ value_json.state }}".to_string()),

                    // Control
                    command_topic: Some(format!("neolink/{}/control/floodlight", cam_config.name)),
                    // Lowercase payloads to match neolink convention
                    payload_on: "on".to_string(),
                    payload_off: "off".to_string(),
                };

                // Each feature needs to be individually registered
                mqtt.send_message_with_root_topic(
                    &format!(
                        "{}/light/{}",
                        discovery_config.topic, &config_data.unique_id
                    ),
                    "config",
                    &serde_json::to_string(&config_data)
                        .with_context(|| "Cound not serialise discovery light config into json")?,
                    true,
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to publish floodlight auto-discover data on over MQTT for {}",
                        cam_config.name
                    )
                })?;
            }
            Discoveries::Camera => {
                let config_data = DiscoveryCamera {
                    // Common across all potential features
                    device: device.clone(),
                    availability: availability.clone(),

                    // Identifiers
                    name: format!("{} Camera", friendly_name.as_str()),
                    unique_id: format!("neolink_{}_camera", cam_config.name),
                    icon: Some("mdi:camera-iris".to_string()),

                    // Camera specific
                    topic: format!("neolink/{}/status/preview", cam_config.name),
                    image_encoding: Encoding::Base64,
                };

                // Each feature needs to be individually registered
                mqtt.send_message_with_root_topic(
                    &format!(
                        "{}/camera/{}",
                        discovery_config.topic, &config_data.unique_id
                    ),
                    "config",
                    &serde_json::to_string(&config_data)
                        .with_context(|| "Cound not serialise discovery camera config into json")?,
                    true,
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to publish camera auto-discover data on over MQTT for {}",
                        cam_config.name
                    )
                })?;
            }
            Discoveries::Led => {
                let config_data = DiscoverySwitch {
                    // Common across all potential features
                    device: device.clone(),
                    availability: availability.clone(),

                    // Identifiers
                    name: format!("{} LED", friendly_name.as_str()),
                    unique_id: format!("neolink_{}_led", cam_config.name),
                    icon: Some("mdi:led-on".to_string()),

                    // Switch specific
                    command_topic: format!("neolink/{}/control/led", cam_config.name),
                    payload_off: "off".to_string(),
                    payload_on: "on".to_string(),
                    state_topic: None,
                    state_off: None,
                    state_on: None,
                };

                // Each feature needs to be individually registered
                mqtt.send_message_with_root_topic(
                    &format!(
                        "{}/switch/{}",
                        discovery_config.topic, &config_data.unique_id
                    ),
                    "config",
                    &serde_json::to_string(&config_data)
                        .with_context(|| "Cound not serialise discovery led config into json")?,
                    true,
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to publish led auto-discover data on over MQTT for {}",
                        cam_config.name
                    )
                })?;
            }
            Discoveries::Ir => {
                let config_data = DiscoverySelect {
                    // Common across all potential features
                    device: device.clone(),
                    availability: availability.clone(),

                    // Identifiers
                    name: format!("{} IR", friendly_name.as_str()),
                    unique_id: format!("neolink_{}_ir", cam_config.name),
                    icon: Some("mdi:lightbulb-night".to_string()),

                    // Switch specific
                    command_topic: format!("neolink/{}/control/ir", cam_config.name),
                    options: vec!["on".to_string(), "off".to_string(), "auto".to_string()],
                    state_topic: None,
                };

                // Each feature needs to be individually registered
                mqtt.send_message_with_root_topic(
                    &format!(
                        "{}/select/{}",
                        discovery_config.topic, &config_data.unique_id
                    ),
                    "config",
                    &serde_json::to_string(&config_data)
                        .with_context(|| "Cound not serialise discovery led config into json")?,
                    true,
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to publish led auto-discover data on over MQTT for {}",
                        cam_config.name
                    )
                })?;
            }
            Discoveries::Motion => {
                let config_data = DiscoveryBinarySensor {
                    // Common across all potential features
                    device: device.clone(),
                    availability: availability.clone(),

                    // Identifiers
                    name: format!("{} MD", friendly_name.as_str()),
                    unique_id: format!("neolink_{}_md", cam_config.name),
                    icon: Some("mdi:motion-sensor".to_string()),

                    // Switch specific
                    state_topic: format!("neolink/{}/status/motion", cam_config.name),
                    payload_off: "off".to_string(),
                    payload_on: "on".to_string(),
                };

                // Each feature needs to be individually registered
                mqtt.send_message_with_root_topic(
                    &format!(
                        "{}/binary_sensor/{}",
                        discovery_config.topic, &config_data.unique_id
                    ),
                    "config",
                    &serde_json::to_string(&config_data)
                        .with_context(|| "Cound not serialise discovery motion config into json")?,
                    true,
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to publish motion auto-discover data on over MQTT for {}",
                        cam_config.name
                    )
                })?;
            }
            Discoveries::Reboot => {
                let config_data = DiscoveryButton {
                    // Common across all potential features
                    device: device.clone(),
                    availability: availability.clone(),

                    // Identifiers
                    name: format!("{} Reboot", friendly_name.as_str()),
                    unique_id: format!("neolink_{}_reboot", cam_config.name),
                    icon: Some("mdi:restart".to_string()),

                    // Switch specific
                    command_topic: format!("neolink/{}/control/reboot", cam_config.name),
                    payload_press: None,
                };

                // Each feature needs to be individually registered
                mqtt.send_message_with_root_topic(
                    &format!(
                        "{}/button/{}",
                        discovery_config.topic, &config_data.unique_id
                    ),
                    "config",
                    &serde_json::to_string(&config_data)
                        .with_context(|| "Cound not serialise discovery reboot config into json")?,
                    true,
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to publish reboot auto-discover data on over MQTT for {}",
                        cam_config.name
                    )
                })?;
            }
            Discoveries::Pt => {
                for dir in ["left", "right", "up", "down"] {
                    let config_data = DiscoveryButton {
                        // Common across all potential features
                        device: device.clone(),
                        availability: availability.clone(),

                        // Identifiers
                        name: format!("{} Pan {}", friendly_name.as_str(), dir),
                        unique_id: format!("neolink_{}_pan_{}", cam_config.name, dir),
                        icon: Some(format!("mdi:pan-{}", dir)),

                        // Switch specific
                        command_topic: format!("neolink/{}/control/ptz", cam_config.name),
                        payload_press: Some(dir.to_string()),
                    };

                    // Each feature needs to be individually registered
                    mqtt.send_message_with_root_topic(
                        &format!(
                            "{}/button/{}",
                            discovery_config.topic, &config_data.unique_id
                        ),
                        "config",
                        &serde_json::to_string(&config_data)
                            .with_context(|| "Cound not serialise discovery pt config into json")?,
                        true,
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to publish pt auto-discover data on over MQTT for {}",
                            cam_config.name
                        )
                    })?;
                }
            }
            Discoveries::Battery => {
                let config_data = DiscoverySensor {
                    // Common across all potential features
                    device: device.clone(),
                    availability: availability.clone(),

                    // Identifiers
                    name: format!("{} Battery", friendly_name.as_str()),
                    unique_id: format!("neolink_{}_battery", cam_config.name),
                    icon: Some("mdi:battery".to_string()),

                    // Camera specific
                    state_topic: format!("neolink/{}/status/battery_level", cam_config.name),
                    state_class: "measurement".to_string(),
                    unit_of_measurement: "%".to_string(),
                };

                // Each feature needs to be individually registered
                mqtt.send_message_with_root_topic(
                    &format!(
                        "{}/sensor/{}",
                        discovery_config.topic, &config_data.unique_id
                    ),
                    "config",
                    &serde_json::to_string(&config_data).with_context(|| {
                        "Cound not serialise discovery battery config into json"
                    })?,
                    true,
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to publish battery auto-discover data on over MQTT for {}",
                        cam_config.name
                    )
                })?;
            }
        }
    }

    info!(
        "Enabled MQTT discovery for {} with friendly name {}",
        cam_config.name,
        friendly_name.as_str()
    );

    Ok(())
}

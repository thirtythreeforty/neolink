//! Enable and configures home assistant MQTTT discovery
//!
//! https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery
//!
use anyhow::{Context, Result};
use heck::ToTitleCase;
use log::*;
use std::sync::Arc;

use super::mqttc::MqttSender;
use crate::config::{CameraConfig, MqttDiscoveryConfig};
use serde::{Serialize, Serializer};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    state_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_value_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_topic: Option<String>,
    payload_on: String,
    payload_off: String,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
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
    topic: String,
    device: DiscoveryDevice,
    availability: DiscoveryAvaliablity,
    #[serde(skip_serializing_if = "Encoding::is_none")]
    image_encoding: Encoding,
}

/// Enables MQTT discovery for a camera. See docs at https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery
pub(crate) async fn enable_discovery(
    discovery_config: &MqttDiscoveryConfig,
    mqtt_sender: &MqttSender,
    cam_config: &Arc<CameraConfig>,
) -> Result<()> {
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
        match feature.as_str() {
            "floodlight" => {
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
                mqtt_sender
                    .send_message_with_root_topic(
                        &format!("{}/light", discovery_config.topic),
                        "config",
                        &serde_json::to_string(&config_data).with_context(|| {
                            "Cound not serialise discovery light config into json"
                        })?,
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
            "camera" => {
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
                mqtt_sender
                    .send_message_with_root_topic(
                        &format!("{}/camera", discovery_config.topic),
                        "config",
                        &serde_json::to_string(&config_data).with_context(|| {
                            "Cound not serialise discovery camera config into json"
                        })?,
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
            _ => {
                error!(
                    "Unsupported MQTT feature {} for {}",
                    feature, cam_config.name
                );
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

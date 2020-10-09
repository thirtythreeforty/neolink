use crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use log::*;
use rumqttc::{
    Client, ClientError, ConnectReturnCode, Connection, Event, Incoming, LastWill, MqttOptions,
    Publish, QoS,
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use validator::{Validate, ValidationError};
use validator_derive::Validate;

pub struct MQTT {
    client: Mutex<Client>,
    connection: Mutex<Connection>,
    name: String,
    incoming: (Sender<Publish>, Receiver<Publish>),
    msg_channel: (Sender<MqttReply>, Receiver<MqttReply>),
    drop_message: Mutex<Option<MqttReply>>,
}

pub struct MqttReply {
    pub topic: String,
    pub message: String,
}

#[derive(Debug, Deserialize, Clone, Validate)]
#[validate(schema(function = "validate_mqtt_config", skip_on_field_errors = true))]
pub struct MqttConfig {
    #[serde(alias = "server")]
    broker_addr: String,

    port: u16,

    #[serde(default)]
    credentials: Option<(String, String)>,

    #[serde(default)]
    ca: Option<std::path::PathBuf>,

    #[serde(default)]
    client_auth: Option<(std::path::PathBuf, std::path::PathBuf)>,
}

fn validate_mqtt_config(config: &MqttConfig) -> Result<(), ValidationError> {
    if config.ca.is_some() && config.client_auth.is_some() {
        Err(ValidationError::new(
            "Cannot have both ca and client_auth set",
        ))
    } else {
        Ok(())
    }
}

impl MQTT {
    pub fn new(config: &MqttConfig, name: &str) -> Arc<Self> {
        let incoming = unbounded::<Publish>();
        let msg_channel = unbounded::<MqttReply>();
        let mut mqttoptions = MqttOptions::new(
            format!("Neolink-{}", name),
            &config.broker_addr,
            config.port,
        );
        if let Some(ca_path) = &config.ca {
            if let Ok(buf) = std::fs::read(ca_path) {
                mqttoptions.set_ca(buf);
            } else {
                error!("Failed to read CA certificate");
            }
        }

        if let Some((cert_path, key_path)) = &config.client_auth {
            if let (Ok(cert_buf), Ok(key_buf)) = (std::fs::read(cert_path), std::fs::read(key_path))
            {
                mqttoptions.set_client_auth(cert_buf, key_buf);
            } else {
                error!("Failed to set client tls");
            }
        }

        if let Some((username, password)) = &config.credentials {
            mqttoptions.set_credentials(username, password);
        }

        mqttoptions.set_keep_alive(5);

        // On unclean disconnect send this
        mqttoptions.set_last_will(LastWill::new(
            format!("neolink/{}/status", name),
            "offline",
            QoS::AtLeastOnce,
            true,
        ));

        let (client, connection) = Client::new(mqttoptions, 10);

        let me = Self {
            client: Mutex::new(client),
            connection: Mutex::new(connection),
            name: name.to_string(),
            incoming,
            msg_channel,
            // Send the drop message on clean disconnect
            drop_message: Mutex::new(Some(MqttReply {
                topic: "status".to_string(),
                message: "offline".to_string(),
            })),
        };

        let arc_me = Arc::new(me);

        // Start the mqtt server
        let mqtt_running = arc_me.clone();
        std::thread::spawn(move || {
            let _ = (*mqtt_running).start();
        });

        // Start polling messages
        let mqtt_reading = arc_me.clone();
        let arc_name = Arc::new(name.to_string());
        std::thread::spawn(move || loop {
            if (*mqtt_reading).handle_message().is_err() {
                error!("Failed to get messages from mqtt client {}", (*arc_name));
            }
        });

        arc_me
    }

    #[allow(unused)]
    pub fn set_drop_message(&mut self, topic: &str, message: &str) {
        self.drop_message.lock().unwrap().replace(MqttReply {
            topic: topic.to_string(),
            message: message.to_string(),
        });
    }

    fn subscribe(&self) -> Result<(), ClientError> {
        let mut client = self.client.lock().unwrap();
        client.subscribe(format!("neolink/{}/#", self.name), QoS::AtMostOnce)?;
        Ok(())
    }

    fn update_status(&self) -> Result<(), ClientError> {
        self.send_message("status", "disconnected", true)?;
        Ok(())
    }

    pub fn send_message(
        &self,
        sub_topic: &str,
        message: &str,
        retain: bool,
    ) -> Result<(), ClientError> {
        let mut client = self.client.lock().unwrap();
        client.publish(
            format!("neolink/{}/{}", self.name, sub_topic),
            QoS::AtLeastOnce,
            retain,
            message,
        )?;
        Ok(())
    }

    fn handle_message(&self) -> Result<(), RecvError> {
        let (_, receiver) = &self.incoming;
        let published_message = receiver.recv()?;

        if let Some(sub_topic) = published_message
            .topic
            .strip_prefix(&format!("neolink/{}/", &self.name))
        {
            if self
                .msg_channel
                .0
                .send(MqttReply {
                    topic: sub_topic.to_string(),
                    message: String::from_utf8_lossy(published_message.payload.as_ref())
                        .into_owned(),
                })
                .is_err()
            {
                error!("Failed to send messages up the mqtt msg channel");
            }
        }

        Ok(())
    }

    pub fn get_message_listener(&self) -> Receiver<MqttReply> {
        self.msg_channel.1.clone()
    }

    pub fn start(&self) -> Result<(), ClientError> {
        // This acts as an event loop
        let mut connection = self.connection.lock().unwrap();
        let (sender, _) = &self.incoming;
        info!("Starting MQTT Client for {}", self.name);
        loop {
            for (_i, notification) in connection.iter().enumerate() {
                if let Ok(notification) = notification {
                    match notification {
                        Event::Incoming(Incoming::ConnAck(connected)) => {
                            if ConnectReturnCode::Accepted == connected.code {
                                self.update_status()?;
                                // We succesfully logged in. Now ask for the cameras subscription.
                                self.subscribe()?;
                            }
                        }
                        Event::Incoming(Incoming::Publish(published_message)) => {
                            if sender.send(published_message).is_err() {
                                error!("Failed to publish motion message on mqtt");
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

impl Drop for MQTT {
    fn drop(&mut self) {
        let drop_message = self.drop_message.lock().unwrap();
        if let Some(drop_message) = drop_message.as_ref() {
            if self
                .send_message(&drop_message.topic, &drop_message.message, true)
                .is_err()
            {
                error!("Failed to send offline message to mqtt");
            }
        }
    }
}

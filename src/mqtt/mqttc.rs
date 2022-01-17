use super::App;
use crate::config::MqttConfig;
use anyhow::{Context, Result};
use crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use log::*;
use rumqttc::{
    Client, ClientError, ConnectReturnCode, Connection, Event, Incoming, Key, LastWill,
    MqttOptions, Publish, QoS, TlsConfiguration, Transport,
};
use std::sync::{Arc, Mutex};

pub(crate) struct Mqtt {
    app: Arc<App>,
    client: Mutex<Client>,
    connection: Mutex<Connection>,
    name: String,
    incoming: (Sender<Publish>, Receiver<Publish>),
    msg_channel: (Sender<MqttReply>, Receiver<MqttReply>),
    drop_message: Mutex<Option<MqttReply>>,
}

pub(crate) struct MqttReply {
    pub(crate) topic: String,
    pub(crate) message: String,
}

impl MqttReply {
    pub(crate) fn as_ref(&self) -> MqttReplyRef {
        MqttReplyRef {
            topic: &self.topic,
            message: &self.message,
        }
    }
}

pub(crate) struct MqttReplyRef<'a> {
    pub(crate) topic: &'a str,
    pub(crate) message: &'a str,
}

impl Mqtt {
    pub(crate) fn new(config: &MqttConfig, name: &str, app: Arc<App>) -> Arc<Self> {
        let incoming = unbounded::<Publish>();
        let msg_channel = unbounded::<MqttReply>();
        let mut mqttoptions = MqttOptions::new(
            format!("Neolink-{}", name),
            &config.broker_addr,
            config.port,
        );

        // Use TLS if ca path is set
        if let Some(ca_path) = &config.ca {
            if let Ok(ca) = std::fs::read(ca_path) {
                // Use client_auth if they have cert and key
                let client_auth = if let Some((cert_path, key_path)) = &config.client_auth {
                    if let (Ok(cert_buf), Ok(key_buf)) =
                        (std::fs::read(cert_path), std::fs::read(key_path))
                    {
                        Some((cert_buf, Key::RSA(key_buf)))
                    } else {
                        error!("Failed to set client tls");
                        None
                    }
                } else {
                    None
                };

                let transport = Transport::Tls(TlsConfiguration::Simple {
                    ca,
                    alpn: None,
                    client_auth,
                });
                mqttoptions.set_transport(transport);
            } else {
                error!("Failed to set CA");
            }
        };

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
            app: app.clone(),
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
        let mqtt_app = app.clone();
        let mqtt_name = name.to_string();
        std::thread::spawn(move || {
            let _ = (*mqtt_running).start();
            mqtt_app.abort(&mqtt_name)
        });

        // Start polling messages
        info!("{}: Starting listening to mqtt", name);
        let mqtt_read_app = app;
        let mqtt_reading = arc_me.clone();
        let mqtt_read_name = name.to_string();
        std::thread::spawn(move || {
            while mqtt_read_app.running(&format!("app:{}", mqtt_read_name)) {
                if (*mqtt_reading).handle_message().is_err() {
                    error!(
                        "Failed to get messages from mqtt client {}",
                        (mqtt_read_name)
                    );
                }
            }
        });

        arc_me
    }

    #[allow(unused)]
    pub(crate) fn set_drop_message(&mut self, topic: &str, message: &str) {
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

    pub(crate) fn poll(&self) -> Result<MqttReply> {
        self.get_message_listener()
            .recv()
            .context("Mqtt Polling error")
    }

    pub(crate) fn start(&self) -> Result<()> {
        // This acts as an event loop
        let mut connection = self.connection.lock().unwrap();
        let (sender, _) = &self.incoming;
        info!("Starting MQTT Client for {}", self.name);
        while self.app.running(&format!("app:{}", self.name)) {
            for (_i, notification) in connection.iter().enumerate() {
                if let Ok(notification) = notification {
                    match notification {
                        Event::Incoming(Incoming::ConnAck(connected)) => {
                            if ConnectReturnCode::Success == connected.code {
                                self.update_status().context("Failed to update status")?;
                                // We succesfully logged in. Now ask for the cameras subscription.
                                self.subscribe().context("Failed to subscribe")?;
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
        Ok(())
    }
}

impl Drop for Mqtt {
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

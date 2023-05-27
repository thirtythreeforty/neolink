use crate::config::MqttConfig;
use anyhow::{anyhow, Context, Result};
use log::*;
use rumqttc::{
    AsyncClient, ClientError, ConnectReturnCode, Event, EventLoop, Incoming, Key, LastWill,
    MqttOptions, Publish, QoS, TlsConfiguration, Transport,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinSet;

pub(crate) struct Mqtt {
    client: Arc<AsyncClient>,
    name: String,
    incoming: Receiver<MqttReply>,
    set: JoinSet<Result<()>>,
    drop_message: Option<MqttReply>,
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

struct MqttReciever {
    client: MqttSender,
    incoming_tx: Sender<MqttReply>,
    name: String,
}

#[derive(Clone)]
pub(crate) struct MqttSender {
    client: Arc<AsyncClient>,
    name: String,
}

impl MqttSender {
    pub async fn send_message_with_root_topic(
        &self,
        root_topic: &str,
        sub_topic: &str,
        message: &str,
        retain: bool,
    ) -> Result<(), ClientError> {
        self.client
            .publish(
                format!("{}/{}/{}", root_topic, self.name, sub_topic),
                QoS::AtLeastOnce,
                retain,
                message,
            )
            .await?;
        Ok(())
    }

    pub async fn send_message(
        &self,
        sub_topic: &str,
        message: &str,
        retain: bool,
    ) -> Result<(), ClientError> {
        self.send_message_with_root_topic("neolink", sub_topic, message, retain)
            .await?;
        Ok(())
    }

    async fn subscribe(&self) -> Result<(), ClientError> {
        self.client
            .subscribe(format!("neolink/{}/#", self.name), QoS::AtMostOnce)
            .await?;
        Ok(())
    }

    async fn update_status(&self) -> Result<()> {
        self.send_message("status", "disconnected", true).await?;
        Ok(())
    }

    pub fn try_send_message(&self, sub_topic: &str, message: &str, retain: bool) -> Result<()> {
        self.client
            .try_publish(
                format!("neolink/{}/{}", self.name, sub_topic),
                QoS::AtLeastOnce,
                retain,
                message,
            )
            .map_err(|e| e.into())
    }
}

impl MqttReciever {
    async fn run(&mut self, connection: &mut EventLoop) -> Result<()> {
        // This acts as an event loop
        let name = self.name.clone();
        info!("Starting MQTT Client for {}", name);
        loop {
            let notification = connection
                .poll()
                .await
                .with_context(|| "MQTT connection dropped")?;
            match notification {
                Event::Incoming(Incoming::ConnAck(connected)) => {
                    if ConnectReturnCode::Success == connected.code {
                        self.client
                            .update_status()
                            .await
                            .context("Failed to update status")?;
                        // We succesfully logged in. Now ask for the cameras subscription.
                        self.client
                            .subscribe()
                            .await
                            .context("Failed to subscribe")?;
                    }
                }
                Event::Incoming(Incoming::Publish(published_message)) => {
                    if self.handle_message(published_message).await.is_err() {
                        error!("Failed to forward messages in mqtt");
                    }
                }
                _ => {}
            }
        }
    }

    async fn handle_message(&mut self, published_message: Publish) -> Result<()> {
        if let Some(sub_topic) = published_message
            .topic
            .strip_prefix(&format!("neolink/{}/", &self.name))
        {
            if self
                .incoming_tx
                .send(MqttReply {
                    topic: sub_topic.to_string(),
                    message: String::from_utf8_lossy(published_message.payload.as_ref())
                        .into_owned(),
                })
                .await
                .is_err()
            {
                error!("Failed to send messages up the mqtt msg channel");
            }
        }

        Ok(())
    }
}

impl Mqtt {
    pub(crate) async fn new(config: &MqttConfig, name: &str) -> Self {
        let (incoming_tx, incoming) = channel::<MqttReply>(100);
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

        mqttoptions.set_keep_alive(Duration::from_secs(5));

        // On unclean disconnect send this
        mqttoptions.set_last_will(LastWill::new(
            format!("neolink/{}/status", name),
            "offline",
            QoS::AtLeastOnce,
            true,
        ));

        let (client, mut connection) = AsyncClient::new(mqttoptions, 10);

        let client = Arc::new(client);

        // Start the mqtt server
        let mut set = JoinSet::<Result<()>>::new();

        let mut reciever = MqttReciever {
            incoming_tx,
            name: name.to_string(),
            client: MqttSender {
                client: client.clone(),
                name: name.to_string(),
            },
        };
        set.spawn(async move { reciever.run(&mut connection).await });

        Self {
            client,
            name: name.to_string(),
            incoming,
            set,
            // Send the drop message on clean disconnect
            drop_message: Some(MqttReply {
                topic: "status".to_string(),
                message: "offline".to_string(),
            }),
        }
    }

    pub fn get_sender(&self) -> MqttSender {
        MqttSender {
            client: self.client.clone(),
            name: self.name.to_string(),
        }
    }

    /// This will also error is the join set errors
    pub(crate) async fn poll(&mut self) -> Result<MqttReply> {
        let (incoming, set) = (&mut self.incoming, &mut self.set);
        tokio::select! {
            v = incoming.recv() => v.with_context(|| "Mqtt Polling error"),
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
                Err(anyhow!("MQTT background thread dropped without error"))
            } => v.with_context(|| "MQTT Threads aborted"),
        }
    }
}

impl Drop for Mqtt {
    fn drop(&mut self) {
        if let Some(drop_message) = self.drop_message.as_ref() {
            let res = self.get_sender().try_send_message(
                &drop_message.topic,
                &drop_message.message,
                true,
            );
            if res.is_err() {
                error!(
                    "Failed to send offline message to mqtt: {:?}. Is the MQTT topic name valid?",
                    res.err().unwrap()
                );
            }
        }
        self.set.abort_all();
    }
}

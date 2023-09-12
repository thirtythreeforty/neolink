use crate::{
    config::{Config, MqttServerConfig},
    AnyResult,
};
use anyhow::{anyhow, Context, Result};
use futures::future::FutureExt;
use log::*;
use rumqttc::{
    AsyncClient, ConnectReturnCode, Event, Incoming, Key, LastWill, MqttOptions, QoS,
    TlsConfiguration, Transport,
};
use std::sync::Arc;
use tokio::task::JoinSet;
use tokio::{
    sync::{
        broadcast::{channel as broadcast, Sender as BroadcastSender},
        mpsc::{channel as mpsc, Receiver as MpscReceiver, Sender as MpscSender},
        oneshot::{channel as oneshot, Sender as OneshotSender},
        watch::Receiver as WatchReceiver,
    },
    time::{sleep, Duration},
};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tokio_util::sync::CancellationToken;

pub(crate) struct Mqtt {
    cancel: CancellationToken,
    outgoing_tx: MpscSender<MqttRequest>,
    set: JoinSet<Result<()>>,
}

impl Mqtt {
    pub(crate) async fn new(config: WatchReceiver<Config>) -> Self {
        let (incoming_tx, _) = broadcast::<MqttReply>(100);
        let (outgoing_tx, mut outgoing_rx) = mpsc::<MqttRequest>(100);
        let cancel = CancellationToken::new();
        let mut set = JoinSet::<AnyResult<()>>::new();

        // Thread that handles the mqttc side
        // including restarting it if the config changes
        let thread_cancel = cancel.clone();
        let mut thread_config = config;
        let thread_incoming_tx = incoming_tx;
        let thread_outgoing_tx = outgoing_tx.clone();
        set.spawn(async move {
            let mut mqtt_config = thread_config.borrow().mqtt.clone();
            loop {
                break tokio::select! {
                    _ = thread_cancel.cancelled() => AnyResult::Ok(()),
                    v = thread_config.wait_for(|config| config.mqtt != mqtt_config).map(|res| res.map(|r| r.clone())) =>
                    {
                        mqtt_config = v?.mqtt.clone();
                        continue;
                    }
                    v = async {
                        run_mqtt_server(thread_incoming_tx.clone(), &mut outgoing_rx, thread_outgoing_tx.clone(), mqtt_config.as_ref().unwrap()).await
                    }, if mqtt_config.is_some() => {
                        if let Err(e) = &v {
                            log::error!("MQTT Client Connection Failed: {:?}", e);
                            sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                        v
                    },
                };
            }
        });

        Self {
            cancel,
            outgoing_tx,
            set,
            // Send the drop message on clean disconnect
        }
    }

    pub async fn subscribe<T: Into<String>>(&self, name: T) -> AnyResult<MqttInstance> {
        let (tx, rx) = oneshot();
        self.outgoing_tx
            .send(MqttRequest::Subscribe(name.into(), tx))
            .await?;
        rx.await?
    }
}

async fn run_mqtt_server(
    incomming_tx: BroadcastSender<MqttReply>,
    outgoing_rx: &mut MpscReceiver<MqttRequest>,
    outgoing_tx: MpscSender<MqttRequest>,
    config: &MqttServerConfig,
) -> AnyResult<()> {
    let mut mqttoptions = MqttOptions::new("Neolink".to_string(), &config.broker_addr, config.port);
    let max_size = 100 * (1024 * 1024);
    mqttoptions.set_max_packet_size(max_size, max_size);

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
        "neolink/status".to_string(),
        "offline",
        QoS::AtLeastOnce,
        true,
    ));

    let (client, mut connection) = AsyncClient::new(mqttoptions, 10);

    let client = Arc::new(client);
    let send_client = client.clone();
    send_client
        .publish(
            "neolink/status".to_string(),
            QoS::AtLeastOnce,
            true,
            "connected".to_string(),
        )
        .await?;
    loop {
        let r = tokio::select! {
            v = async {
                let msg = outgoing_rx.recv().await.ok_or(anyhow!("All outgoing MQTT channels closed"))?;
                match msg {
                    MqttRequest::Send(msg) =>  {
                        send_client.publish(
                            msg.topic,
                            QoS::AtLeastOnce,
                            false,
                            msg.message,
                        ).await?;
                    }
                    MqttRequest::SendRetained(msg) =>  {
                        send_client.publish(
                            msg.topic,
                            QoS::AtLeastOnce,
                            true,
                            msg.message,
                        ).await?;
                    }
                    MqttRequest::HangUp(reply) => {
                        send_client.publish(
                            "neolink/status".to_string(),
                            QoS::AtLeastOnce,
                            true,
                            "disconnected".to_string(),
                        ).await?;
                        let _ = reply.send(());
                    }
                    MqttRequest::Subscribe(name, reply) => {
                        let instance = MqttInstance {
                            name,
                            incomming_rx: BroadcastStream::new(incomming_tx.subscribe()),
                            outgoing_tx: outgoing_tx.clone(),
                        };
                        let _ = reply.send(Ok(instance));
                    }
                }

                AnyResult::Ok(())
            } => {
                v
            },
            v = async {
                let notification = connection
                    .poll()
                    .await
                    .with_context(|| "MQTT connection dropped")?;
                match notification {
                    Event::Incoming(Incoming::ConnAck(connected)) => {
                        if ConnectReturnCode::Success == connected.code {
                            // Publish disconnected at first then wait for connect from the camera insances
                            client
                            .publish(
                                "neolink/status".to_string(),
                                QoS::AtLeastOnce,
                                true,
                                "disconnected",
                            )
                            .await?;
                            // We succesfully logged in. Now ask for the cameras subscription.
                            client
                            .subscribe("neolink/".to_string(), QoS::AtMostOnce)
                            .await?;
                        }
                    }
                    Event::Incoming(Incoming::Publish(published_message)) => {
                        if let Some(sub_topic) = published_message
                            .topic
                            .strip_prefix("neolink/")
                        {
                            let _ = incomming_tx
                                .send(MqttReply {
                                    topic: sub_topic.to_string(),
                                    message: String::from_utf8_lossy(published_message.payload.as_ref())
                                        .into_owned(),
                                });
                        }
                    }
                    _ => {}
                }
                AnyResult::Ok(())
            } => {
                v
            },
        };
        if r.is_ok() {
            continue;
        }
        break r;
    }?;
    Ok(())
}

impl Drop for Mqtt {
    fn drop(&mut self) {
        tokio::task::block_in_place(move || {
            let _ = tokio::runtime::Handle::current().block_on(async move {
                let (tx, rx) = oneshot();
                let _ = self.outgoing_tx.send(MqttRequest::HangUp(tx)).await;
                let _ = rx.await;

                self.cancel.cancel();
                while self.set.join_next().await.is_some() {}
                AnyResult::Ok(())
            });
        });
    }
}

pub(crate) struct MqttInstance {
    outgoing_tx: MpscSender<MqttRequest>,
    incomming_rx: BroadcastStream<MqttReply>,
    name: String,
}

impl MqttInstance {
    pub async fn subscribe<T: Into<String>>(&self, name: T) -> AnyResult<Self> {
        let (tx, rx) = oneshot();
        self.outgoing_tx
            .send(MqttRequest::Subscribe(name.into(), tx))
            .await?;
        rx.await?
    }

    pub async fn resubscribe(&self) -> AnyResult<Self> {
        let (tx, rx) = oneshot();
        self.outgoing_tx
            .send(MqttRequest::Subscribe(self.name.clone(), tx))
            .await?;
        rx.await?
    }

    pub async fn send_message_with_root_topic(
        &self,
        root_topic: &str,
        sub_topic: &str,
        message: &str,
        retain: bool,
    ) -> AnyResult<()> {
        let topics = vec![
            root_topic.to_string(),
            self.name.clone(),
            sub_topic.to_string(),
        ]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>();
        if retain {
            self.outgoing_tx
                .send(MqttRequest::SendRetained(MqttReply {
                    topic: topics.join("/"),
                    message: message.to_string(),
                }))
                .await?;
        } else {
            self.outgoing_tx
                .send(MqttRequest::Send(MqttReply {
                    topic: topics.join("/"),
                    message: message.to_string(),
                }))
                .await?;
        }
        Ok(())
    }

    pub async fn send_message(
        &self,
        sub_topic: &str,
        message: &str,
        retain: bool,
    ) -> AnyResult<()> {
        self.send_message_with_root_topic("neolink", sub_topic, message, retain)
            .await?;
        Ok(())
    }

    pub(crate) async fn recv(&mut self) -> AnyResult<MqttReply> {
        Ok(loop {
            let msg = self
                .incomming_rx
                .next()
                .await
                .ok_or(anyhow!("End or client data"))?
                .with_context(|| "Clinet stream is too far behind")?;

            if self.name.is_empty() {
                break msg;
            } else {
                let topics = msg.topic.split('/').collect::<Vec<_>>();
                if topics
                    .get(2)
                    .map(|subtopic| *subtopic == self.name)
                    .unwrap_or(false)
                {
                    break msg;
                }
            }
        })
    }
}

#[derive(Clone)]
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

enum MqttRequest {
    Send(MqttReply),
    SendRetained(MqttReply),
    HangUp(OneshotSender<()>),
    Subscribe(String, OneshotSender<Result<MqttInstance>>),
}

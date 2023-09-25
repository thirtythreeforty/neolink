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
                        let mut backend = MqttBackend {
                            incomming_tx: thread_incoming_tx.clone(),
                            outgoing_rx: &mut outgoing_rx,
                            outgoing_tx: thread_outgoing_tx.clone(),
                            config: mqtt_config.as_ref().unwrap(),
                            cancel: CancellationToken::new(),
                        };
                        backend.run().await
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

impl Drop for Mqtt {
    fn drop(&mut self) {
        log::trace!("Drop MQTT");
        let outgoing_tx = self.outgoing_tx.clone();
        let cancel = self.cancel.clone();
        let mut set = std::mem::take(&mut self.set);

        let _gt = tokio::runtime::Handle::current().enter();
        tokio::task::spawn(async move {
            let (tx, rx) = oneshot();
            let _ = outgoing_tx.send(MqttRequest::HangUp(tx)).await;
            let _ = rx.await;

            log::debug!("Mqtt::drop Cancel");
            cancel.cancel();
            while set.join_next().await.is_some() {}
            log::trace!("Dropped MQTT");
        });
    }
}

struct MqttBackend<'a> {
    incomming_tx: BroadcastSender<MqttReply>,
    outgoing_rx: &'a mut MpscReceiver<MqttRequest>,
    outgoing_tx: MpscSender<MqttRequest>,
    config: &'a MqttServerConfig,
    cancel: CancellationToken,
}

impl<'a> MqttBackend<'a> {
    async fn run(&mut self) -> AnyResult<()> {
        log::trace!("Run MQTT Server");
        let mut mqttoptions = MqttOptions::new(
            "Neolink".to_string(),
            &self.config.broker_addr,
            self.config.port,
        );
        let max_size = 100 * (1024 * 1024);
        mqttoptions.set_max_packet_size(max_size, max_size);

        // Use TLS if ca path is set
        if let Some(ca_path) = &self.config.ca {
            if let Ok(ca) = std::fs::read(ca_path) {
                // Use client_auth if they have cert and key
                let client_auth = if let Some((cert_path, key_path)) = &self.config.client_auth {
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

        if let Some((username, password)) = &self.config.credentials {
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

        let (client, mut connection) = AsyncClient::new(mqttoptions, 100);

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
        log::debug!("MQTT Published Startup");
        let loop_cancel = CancellationToken::new();
        let _drop_guard = loop_cancel.clone().drop_guard();
        loop {
            let r = tokio::select! {
                v = self.outgoing_rx.recv() => {
                    let msg = v.ok_or(anyhow!("All outgoing MQTT channels closed"))?;

                    // Put it on a thread so that we don't block polling
                    let outgoing_tx = self.outgoing_tx.clone();
                    let incomming_tx = self.incomming_tx.clone();
                    let send_client = send_client.clone();
                    let cancel = self.cancel.clone();
                    let thread_cancel = loop_cancel.clone();
                    tokio::task::spawn(async move {
                        tokio::select!{
                            _ = cancel.cancelled() => AnyResult::Ok(()),
                            _ = thread_cancel.cancelled() => AnyResult::Ok(()),
                            v = async {
                                match msg {
                                    MqttRequest::Send(msg, tx) =>  {
                                        let v = send_client.publish(
                                            msg.topic.clone(),
                                            QoS::AtLeastOnce,
                                            false,
                                            (*msg.message).clone(),
                                        ).await;
                                        match &v {
                                            Ok(()) => {
                                                let _ = tx.send(Ok(()));
                                            },
                                            Err(rumqttc::ClientError::Request(_)) | Err(rumqttc::ClientError::TryRequest(_)) => {
                                                // Requeue it
                                                outgoing_tx.send(MqttRequest::Send(msg, tx)).await?;
                                            }
                                        };
                                        v?;
                                    }
                                    MqttRequest::SendRetained(msg, tx) =>  {
                                        let v = send_client.publish(
                                            msg.topic.clone(),
                                            QoS::AtLeastOnce,
                                            true,
                                            (*msg.message).clone(),
                                        ).await;
                                        match &v {
                                            Ok(()) => {
                                                let _ = tx.send(Ok(()));
                                            },
                                            Err(rumqttc::ClientError::Request(_)) | Err(rumqttc::ClientError::TryRequest(_)) => {
                                                // Requeue it
                                                outgoing_tx.send(MqttRequest::Send(msg, tx)).await?;
                                            }
                                        };
                                        v?;
                                    }
                                    MqttRequest::HangUp(reply) => {
                                        send_client.publish(
                                            "neolink/status".to_string(),
                                            QoS::AtLeastOnce,
                                            true,
                                            "disconnected".to_string(),
                                        ).await?;
                                        let _ = reply.send(());
                                        return Err(anyhow!("Disconneting"));
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
                            } => v,
                        }
                    });

                    AnyResult::Ok(())
                },
                v = connection.poll() =>  {
                    let  notification = v.with_context(|| "MQTT connection dropped")?;
                    // Handle message on another thread so that we can keep polling
                    let client = client.clone();
                    let incomming_tx = self.incomming_tx.clone();
                    let cancel = self.cancel.clone();
                    let thread_cancel = loop_cancel.clone();
                    tokio::task::spawn(async move {
                        tokio::select!{
                            _ = cancel.cancelled() => AnyResult::Ok(()),
                            _ = thread_cancel.cancelled() => AnyResult::Ok(()),
                            v = async {
                                match notification {
                                    Event::Incoming(Incoming::ConnAck(connected)) => {
                                        if ConnectReturnCode::Success == connected.code {
                                            // Publish connected now that we are online
                                            client
                                            .publish(
                                                "neolink/status".to_string(),
                                                QoS::AtLeastOnce,
                                                true,
                                                "connected",
                                            )
                                            .await?;
                                            // We succesfully logged in. Now ask for the cameras subscription.
                                            client
                                            .subscribe("neolink/#".to_string(), QoS::AtMostOnce)
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
                                                    message: Arc::new(String::from_utf8_lossy(published_message.payload.as_ref())
                                                        .into_owned()),
                                                });
                                        }
                                    }
                                    _ => {}
                                }
                                AnyResult::Ok(())
                            } => v
                        }
                    });
                    AnyResult::Ok(())
                },
            };
            if r.is_ok() {
                continue;
            }
            break r;
        }?;
        Ok(())
    }
}

impl<'a> Drop for MqttBackend<'a> {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub(crate) struct MqttInstance {
    outgoing_tx: MpscSender<MqttRequest>,
    incomming_rx: BroadcastStream<MqttReply>,
    name: String,
}

impl MqttInstance {
    pub(crate) fn get_name(&self) -> &str {
        &self.name
    }

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
            let (tx, rx) = oneshot();
            self.outgoing_tx
                .send(MqttRequest::SendRetained(
                    MqttReply {
                        topic: topics.join("/"),
                        message: Arc::new(message.to_string()),
                    },
                    tx,
                ))
                .await?;
            rx.await??;
        } else {
            let (tx, rx) = oneshot();
            self.outgoing_tx
                .send(MqttRequest::Send(
                    MqttReply {
                        topic: topics.join("/"),
                        message: Arc::new(message.to_string()),
                    },
                    tx,
                ))
                .await?;
            rx.await??;
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
            let mut msg = self
                .incomming_rx
                .next()
                .await
                .ok_or(anyhow!("End of client data"))?
                .with_context(|| "Client stream is too far behind")?;
            // log::debug!("Got MQTT: {msg:?}");
            // log::debug!("self.name: {:?}", self.name);

            if self.name.is_empty() {
                break msg;
            } else {
                let mut topics = msg.topic.split('/');
                let sub_topic = topics.next();
                // log::debug!("topics: {:?}", msg.topic);
                // log::debug!("sub_topic: {sub_topic:?}");
                if sub_topic
                    .map(|subtopic| *subtopic == self.name)
                    .unwrap_or(false)
                {
                    msg.topic = topics.collect::<Vec<_>>().join("/");
                    // log::debug!("new topics: {:?}", msg.topic);
                    break msg;
                }
            }
        })
    }

    pub(crate) async fn drop_guard_message(
        &self,
        topic: &str,
        message: &str,
    ) -> AnyResult<DropSender> {
        Ok(DropSender {
            instance: Some(self.resubscribe().await?),
            topic: topic.to_string(),
            message: message.to_string(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MqttReply {
    pub(crate) topic: String,
    pub(crate) message: Arc<String>, // Messages can be long so avoid costly clones with an arc
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
    Send(MqttReply, OneshotSender<Result<()>>),
    SendRetained(MqttReply, OneshotSender<Result<()>>),
    HangUp(OneshotSender<()>),
    Subscribe(String, OneshotSender<Result<MqttInstance>>),
}

pub(crate) struct DropSender {
    instance: Option<MqttInstance>,
    topic: String,
    message: String,
}

impl Drop for DropSender {
    fn drop(&mut self) {
        if let Some(instance) = self.instance.take() {
            let _gt = tokio::runtime::Handle::current().enter();
            let topic = self.topic.clone();
            let message = self.message.clone();
            tokio::task::spawn(async move { instance.send_message(&topic, &message, true).await });
        }
    }
}

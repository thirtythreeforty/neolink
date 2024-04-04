//! This thread handles push notifications
//! the last notification is pushed into a watcher
//! as is, which comes fromt the json structure
//!

use anyhow::Context;
use fcm_push_listener::*;
use std::{fs, sync::Arc};
use tokio::{
    sync::{
        mpsc::{Receiver as MpscReceiver, Sender as MpscSender},
        oneshot::Sender as OneshotSender,
        watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
    },
    time::{sleep, timeout, Duration},
};

use super::NeoInstance;
use crate::AnyResult;

pub(crate) struct PushNotiThread {
    pn_watcher: Arc<WatchSender<Option<PushNoti>>>,
    registed_cameras: Vec<NeoInstance>,
    received_ids: Vec<String>,
}

// The push notification
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) struct PushNoti {
    pub(crate) message: String,
    pub(crate) id: Option<String>,
}

pub(crate) enum PnRequest {
    Get {
        sender: OneshotSender<WatchReceiver<Option<PushNoti>>>,
    },
    Activate {
        instance: NeoInstance,
        sender: OneshotSender<AnyResult<()>>,
    },
    AddPushID {
        id: String,
    },
}

impl PushNotiThread {
    pub(crate) async fn new() -> AnyResult<Self> {
        let (pn_watcher, _) = watch(None);

        Ok(PushNotiThread {
            pn_watcher: Arc::new(pn_watcher),
            registed_cameras: vec![],
            received_ids: vec![],
        })
    }

    pub(crate) async fn run(
        &mut self,
        sender: &MpscSender<PnRequest>,
        pn_request_rx: &mut MpscReceiver<PnRequest>,
    ) -> AnyResult<()> {
        loop {
            // Short wait on start/retry
            sleep(Duration::from_secs(3)).await;

            let sender_id = "743639030586"; // andriod
                                            // let sender_id = "696841269229"; // ios

            // let firebase_app_id = "1:743639030586:android:86f60a4fb7143876";
            // let firebase_project_id = "reolink-login";
            // let firebase_api_key = "AIzaSyBEUIuWHnnOEwFahxWgQB4Yt4NsgOmkPyE";
            // let vapid_key = "????";

            let token_path = dirs::config_dir().map(|mut d| {
                d.push("./neolink_token.toml");
                d
            });
            log::debug!("Push notification details are saved to {:?}", token_path);

            let registration = if let Some(Ok(Ok(registration))) =
                token_path.as_ref().map(|token_path| {
                    fs::read_to_string(token_path).map(|v| toml::from_str::<Registration>(&v))
                }) {
                log::debug!("Loaded push notification token");
                registration
            } else {
                log::debug!("Registering new push notification token");
                match fcm_push_listener::register(sender_id).await {
                    Ok(registration) => {
                        let new_token = toml::to_string(&registration)
                            .with_context(|| "Unable to serialise fcm token")?;
                        if let Some(Err(e)) = token_path
                            .as_ref()
                            .map(|token_path| fs::write(token_path, &new_token))
                        {
                            log::warn!(
                                "Unable to save push notification details ({}) to {:#?} because of the error {:#?}",
                                new_token,
                                token_path,
                                e
                            );
                        }
                        registration
                    }
                    Err(e) => {
                        log::warn!("Issue connecting to push notifications server: {:?}", e);
                        continue;
                    }
                }
            };

            // Send registration.fcm_token to the server to allow it to send push messages to you.
            log::debug!("registration.fcm_token: {}", registration.fcm_token);
            let md5ed = md5::compute(format!("WHY_REOLINK_{:?}", registration.fcm_token));
            let uid = format!("{:X}", md5ed);
            let fcm_token = registration.fcm_token.clone();
            log::debug!("push notification UID: {}", uid);

            log::debug!("Push notification Listening");
            let thread_pn_watcher = self.pn_watcher.clone();
            let mut listener = FcmPushListener::create(
                registration,
                |message: FcmMessage| {
                    log::debug!("Got FCM Message: {:?}", message.payload_json);
                    if let Some(id) = message.persistent_id.clone() {
                        // Don't worry if queue is full we will just not register as received yet
                        let _ = sender.try_send(PnRequest::AddPushID { id });
                    }
                    thread_pn_watcher.send_replace(Some(PushNoti {
                        message: message.payload_json,
                        id: message.persistent_id,
                    }));
                },
                self.received_ids.clone(),
            );

            for instance in self.registed_cameras.iter() {
                let uid = uid.clone();
                let fcm_token = fcm_token.clone();
                let instance = instance.clone();
                tokio::task::spawn(async move {
                    let _ = instance
                        .run_task(|camera| {
                            let fcm_token = fcm_token.clone();
                            let uid = uid.clone();
                            Box::pin(async move {
                                let r = camera.send_pushinfo_android(&fcm_token, &uid).await;
                                log::debug!(
                                    "Registered {} for push notifications: {:?}",
                                    camera.uid().await?,
                                    r
                                );
                                r?;
                                AnyResult::Ok(())
                            })
                        })
                        .await;
                });
            }

            tokio::select! {
                v = async {
                    loop {
                        let r = timeout(Duration::from_secs(60*5), listener.connect()).await;
                        match &r {
                            Ok(Ok(_)) => {
                                log::debug!("Push notification listener reported normal shutdown");
                            }
                            Ok(Err(e)) => {
                                use fcm_push_listener::Error::*;
                                match &e {
                                    MissingMessagePayload | MissingCryptoMetadata | ProtobufDecode(_) | Base64Decode(_) => {
                                        // Wipe data so next call is a new token
                                        token_path.map(|token_path|
                                            fs::write(token_path, "")
                                        );
                                        log::debug!("Error on push notification listener: {:?}. Clearing token", e);
                                    },
                                    Http(e) if e.is_request() || e.is_connect() || e.is_timeout() => {
                                        log::debug!("Error on push notification listener: {:?}", e);
                                    }
                                    _ => {
                                        log::debug!("Error on push notification listener: {:?}", e);
                                        // This sort of error can be a network error
                                        // Wait for a little longer
                                        sleep(Duration::from_secs(30)).await;
                                    }
                                }
                            },
                            Err(_) => {
                                // timeout
                                continue;
                            }
                        };
                        break;
                    }
                } => v,
                v = async {
                    while let Some(msg) = pn_request_rx.recv().await {
                        match msg {
                            PnRequest::Get{sender} => {
                                let _ = sender.send(self.pn_watcher.subscribe());
                            }
                            PnRequest::Activate{instance, sender} => {
                                let uid = uid.clone();
                                let fcm_token = fcm_token.clone();
                                self.registed_cameras.push(instance.clone());
                                tokio::task::spawn(async move {
                                    let r = instance.run_task(|camera| {
                                        let fcm_token = fcm_token.clone();
                                        let uid = uid.clone();
                                        Box::pin(async move {
                                            let r = camera.send_pushinfo_android(&fcm_token, &uid).await;
                                            log::debug!(
                                                "Registered {} for push notifications: {:?}",
                                                camera.uid().await?,
                                                r
                                            );
                                            r?;
                                            AnyResult::Ok(())
                                        })
                                    }).await;
                                    let _ = sender.send(r);
                                });
                            }
                            PnRequest::AddPushID{id} => {
                                self.received_ids.push(id);
                            }
                        }
                    }
                } => {
                    // These are critical errors
                    log::debug!("Push Notification thread ended {:?}", v);
                    break AnyResult::Ok(());
                },
            };
        }
    }
}

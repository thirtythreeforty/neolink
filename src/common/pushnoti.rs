//! This thread handles push notifications
//! the last notification is pushed into a watcher
//! as is, which comes fromt the json structure
//!

use fcm_push_listener::*;
use std::{fs, sync::Arc};
use tokio::{
    sync::{
        mpsc::Receiver as MpscReceiver,
        oneshot::Sender as OneshotSender,
        watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
    },
    time::{sleep, Duration},
};

use super::NeoInstance;
use crate::AnyResult;

pub(crate) struct PushNotiThread {
    pn_watcher: Arc<WatchSender<Option<PushNoti>>>,
    instance: NeoInstance,
}

// The push notification
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct PushNoti {
    pub(crate) message: String,
    pub(crate) id: Option<String>,
}

pub(crate) enum PnRequest {
    Get {
        sender: OneshotSender<WatchReceiver<Option<PushNoti>>>,
    },
}

impl PushNotiThread {
    pub(crate) async fn new(instance: NeoInstance) -> AnyResult<Self> {
        let (pn_watcher, _) = watch(None);

        Ok(PushNotiThread {
            pn_watcher: Arc::new(pn_watcher),
            instance,
        })
    }

    pub(crate) async fn run(
        &mut self,
        pn_request_rx: &mut MpscReceiver<PnRequest>,
    ) -> AnyResult<()> {
        loop {
            // Short wait on start/retry
            sleep(Duration::from_secs(3)).await;

            let sender_id = "743639030586"; // andriod
                                            // let sender_id = "696841269229"; // ios

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
                        let new_token = toml::to_string(&registration)?;
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
            log::debug!("push notification UID: {}", uid);
            self.instance
                .run_task(|camera| {
                    let uid = uid.clone();
                    let fcm_token = registration.fcm_token.clone();
                    Box::pin(async move {
                        camera.send_pushinfo_android(&fcm_token, &uid).await?;
                        AnyResult::Ok(())
                    })
                })
                .await?;

            log::debug!("Push notification Listening");
            let thread_pn_watcher = self.pn_watcher.clone();
            let mut listener = FcmPushListener::create(
                registration,
                |message: FcmMessage| {
                    thread_pn_watcher.send_replace(Some(PushNoti {
                        message: message.payload_json,
                        id: message.persistent_id,
                    }));
                },
                vec![],
            );
            tokio::select! {
                v = async {
                    let r = listener.connect().await;
                    if let Err(e) = r {
                        use fcm_push_listener::Error::*;
                        match &e {
                            MissingMessagePayload | MissingCryptoMetadata | ProtobufDecode(_) | Base64Decode(_) => {
                                // Wipe data so next call is a new token
                                token_path.map(|token_path|
                                    fs::write(token_path, "")
                                );
                                log::debug!("Error on push notification listener: {:?}. Clearing token", e);
                                AnyResult::Ok(()) // Allow to restart
                            },
                            Http(e) if e.is_request() || e.is_connect() || e.is_timeout() => {
                                log::debug!("Error on push notification listener: {:?}", e);
                                AnyResult::Ok(()) // Allow to restart
                            }
                            _ => {
                                log::warn!("Error on push notification listener: {:?}", e);
                                // Err(e.into()) // Propegate error so it breaks
                                // Wait forever since it will not work
                                // we just leave the push notificaitons as disabled
                                futures::future::pending().await
                            }
                        }
                    } else {
                        AnyResult::Ok(())
                    }
                } => v,
                v = async {
                    while let Some(msg) = pn_request_rx.recv().await {
                        match msg {
                            PnRequest::Get{sender} => {
                                let _ = sender.send(self.pn_watcher.subscribe());
                            }
                        }
                    }
                    AnyResult::Ok(())
                } => {
                    // These are critical errors
                    // break the loop and return
                    log::debug!("Push Notification thread ended {:?}", v);
                    break v;
                },
            }?;
        }
    }
}

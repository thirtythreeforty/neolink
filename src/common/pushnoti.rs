//! This thread handles push notifications
//! the last notification is pushed into a watcher
//! as is, which comes fromt the json structure
//!

use fcm_push_listener::*;
use std::{fs, sync::Arc};
use tokio::sync::{
    mpsc::Receiver as MpscReceiver,
    oneshot::Sender as OneshotSender,
    watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
};

use super::NeoInstance;
use crate::AnyResult;

pub(crate) struct PushNotiThread {
    pn_watcher: Arc<WatchSender<Option<PushNoti>>>,
    pn_request_rx: MpscReceiver<PnRequest>,
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
    pub(crate) async fn new(
        pn_request_rx: MpscReceiver<PnRequest>,
        instance: NeoInstance,
    ) -> AnyResult<Self> {
        let (pn_watcher, _) = watch(None);

        Ok(PushNotiThread {
            pn_watcher: Arc::new(pn_watcher),
            pn_request_rx,
            instance,
        })
    }

    pub(crate) async fn run(&mut self) -> AnyResult<()> {
        let sender_id = "743639030586"; // andriod
                                        // let sender_id = "696841269229"; // ios

        let token_path = dirs::config_dir().map(|mut d| {
            d.push("./neolink_token.toml");
            d
        });

        let registration = if let Some(Ok(Ok(registration))) =
            token_path.as_ref().map(|token_path| {
                fs::read_to_string(token_path).map(|v| toml::from_str::<Registration>(&v))
            }) {
            log::debug!("Loaded push notification token");
            registration
        } else {
            log::debug!("Registering new push notification token");
            let registration = fcm_push_listener::register(sender_id).await?;
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
                listener.connect().await?;
                AnyResult::Ok(())
            } => v,
            v = async {
                while let Some(msg) = self.pn_request_rx.recv().await {
                    match msg {
                        PnRequest::Get{sender} => {
                            let _ = sender.send(self.pn_watcher.subscribe());
                        }
                    }
                }
                AnyResult::Ok(())
            } => v,
        }?;

        AnyResult::Ok(())
    }
}

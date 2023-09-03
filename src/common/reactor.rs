//! This is the highest level to a camera
//! it represents a collection of managed cameras
use anyhow::anyhow;
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};
use tokio::sync::{
    mpsc::{channel as mpsc, Sender as MpscSender},
    oneshot::{channel as oneshot, Sender as OneshotSender},
    watch::{channel as watch, Receiver as WatchReceiver},
};
use tokio_util::sync::CancellationToken;

use super::{NeoCam, NeoInstance};
use crate::{config::Config, Result};

#[allow(clippy::large_enum_variant)]
enum NeoReactorCommand {
    HangUp,
    Config(OneshotSender<WatchReceiver<Config>>),
    Get(String, OneshotSender<Result<Option<NeoInstance>>>),
}

/// Reactor handles the collection of cameras
#[derive(Clone)]
pub(crate) struct NeoReactor {
    cancel: CancellationToken,
    commander: MpscSender<NeoReactorCommand>,
    arc: Arc<()>,
}

impl NeoReactor {
    pub(crate) async fn new(config: Config) -> Self {
        let (commad_tx, mut command_rx) = mpsc(100);
        let me = Self {
            cancel: CancellationToken::new(),
            commander: commad_tx,
            arc: Arc::new(()),
        };

        let (config_tx, _) = watch(config);

        let cancel = me.cancel.clone();
        let cancel2 = me.cancel.clone();
        let config_tx = Arc::new(config_tx);
        tokio::task::spawn(async move {
            let mut instances: HashMap<String, NeoCam> = Default::default();

            let r = tokio::select! {
                _ = cancel.cancelled() => {
                    for instance in instances.values() {
                        instance.shutdown().await;
                    }
                    Ok(())
                },
                v = async {
                    while let Some(command) = command_rx.recv().await {
                        match command {
                            NeoReactorCommand::HangUp =>  {
                                for instance in instances.values() {
                                    instance.shutdown().await;
                                }
                                log::debug!("Cancel:: NeoReactorCommand::HangUp");
                                cancel2.cancel();
                                return Result::<(), anyhow::Error>::Ok(());
                            }
                            NeoReactorCommand::Config(reply) =>  {
                                let _ = reply.send(config_tx.subscribe());
                            }
                            NeoReactorCommand::Get(name, sender) => {
                                let new = match instances.entry(name.clone()) {
                                    Entry::Occupied(occ) => Result::Ok(Some(occ.get().subscribe().await?)),
                                    Entry::Vacant(vac) => {
                                        log::debug!("Inserting new insance");
                                        let current_config: Config = (*config_tx.borrow()).clone();
                                        if let Some(config) = current_config.cameras.iter().find(|cam| cam.name == name).cloned() {
                                            let cam = NeoCam::new(config).await?;
                                            log::debug!("New instance created");
                                            Result::Ok(Some(
                                                vac.insert(
                                                    cam,
                                                )
                                                .subscribe()
                                                .await?
                                            ))
                                        } else {
                                            Result::Ok(None)
                                        }
                                    }
                                };
                                log::debug!("Got instance from reactor");
                                let _ = sender.send(new);
                            },
                        }
                    }
                    Ok(())
                } => v,
            };
            log::info!("Neoreactor thread done: {r:?}");
            r
        });

        me
    }

    /// Get camera by name but do not create
    pub(crate) async fn get(&self, name: &str) -> Result<NeoInstance> {
        let (sender_tx, sender_rx) = oneshot();
        self.commander
            .send(NeoReactorCommand::Get(name.to_string(), sender_tx))
            .await?;

        sender_rx
            .await??
            .ok_or(anyhow!("Camera `{name}` not found in config"))
    }

    pub(crate) async fn config(&self) -> Result<WatchReceiver<Config>> {
        let (sender_tx, sender_rx) = oneshot();
        self.commander
            .send(NeoReactorCommand::Config(sender_tx))
            .await?;

        Ok(sender_rx.await?)
    }

    pub(crate) async fn shutdown(&self) {
        let _ = self.commander.send(NeoReactorCommand::HangUp).await;
        self.cancel.cancelled().await;
    }
}

impl Drop for NeoReactor {
    fn drop(&mut self) {
        if Arc::strong_count(&self.arc) == 1 {
            log::debug!("Cancel:: NeoReactor::drop");
            self.cancel.cancel();
        }
    }
}

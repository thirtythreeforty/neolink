//! This is the highest level to a camera
//! it represents a collection of managed cameras
use anyhow::anyhow;
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};
use tokio::{
    sync::{
        mpsc::{channel as mpsc, Sender as MpscSender},
        oneshot::{channel as oneshot, Sender as OneshotSender},
        watch::{channel as watch, Receiver as WatchReceiver},
    },
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use super::{NeoCam, NeoInstance};
use crate::{config::Config, AnyResult, Result};

#[allow(clippy::large_enum_variant)]
enum NeoReactorCommand {
    HangUp,
    Config(OneshotSender<WatchReceiver<Config>>),
    UpdateConfig(Config, OneshotSender<Result<()>>),
    Get(String, OneshotSender<Result<Option<NeoInstance>>>),
}

/// Reactor handles the collection of cameras
#[derive(Clone)]
pub(crate) struct NeoReactor {
    cancel: CancellationToken,
    commander: MpscSender<NeoReactorCommand>,
    set: Option<Arc<JoinSet<AnyResult<()>>>>,
}

impl NeoReactor {
    pub(crate) async fn new(config: Config) -> Self {
        let (commad_tx, mut command_rx) = mpsc(100);
        let cancel = CancellationToken::new();
        let (config_tx, _) = watch(config);
        let mut set = JoinSet::new();

        let cancel1 = cancel.clone();
        let cancel2 = cancel.clone();
        let config_tx = Arc::new(config_tx);
        set.spawn(async move {
            let mut instances: HashMap<String, NeoCam> = Default::default();

            let r = tokio::select! {
                _ = cancel1.cancelled() => {
                    instances.clear();
                    Ok(())
                },
                v = async {
                    while let Some(command) = command_rx.recv().await {
                        match command {
                            NeoReactorCommand::HangUp =>  {
                                instances.clear();
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
                            NeoReactorCommand::UpdateConfig(new_conf, reply) => {
                                // Shutdown or Notify instances of a change
                                let mut names = new_conf.cameras.iter().filter_map(|cam_conf| cam_conf.enabled.then(|| (cam_conf.name.clone(), cam_conf.clone()))).collect::<HashMap<_,_>>();
                                // Remove those no longer in the config
                                instances.retain(|name, _| names.contains_key(name));
                                for (name, instance) in instances.iter() {
                                    if let Some(conf) = names.remove(name) {
                                        let _ = instance.update_config(conf).await;
                                    }
                                }

                                // Set the new conf
                                let _ = config_tx.send_replace(new_conf);
                                // Reply that we are done
                                let _ = reply.send(Ok(()));
                            }
                        }
                    }
                    Ok(())
                } => v,
            };
            r
        });

        Self {
            cancel,
            commander: commad_tx,
            set: Some(Arc::new(set)),
        }
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

    pub(crate) async fn update_config(&self, new_config: Config) -> Result<()> {
        let (sender_tx, sender_rx) = oneshot();
        self.commander
            .send(NeoReactorCommand::UpdateConfig(new_config, sender_tx))
            .await?;

        sender_rx.await?
    }
}

impl Drop for NeoReactor {
    fn drop(&mut self) {
        if let Some(set) = self.set.take() {
            if let Ok(mut set) = Arc::try_unwrap(set) {
                log::trace!("Drop NeoReactor");
                self.cancel.cancel();
                let commander = self.commander.clone();
                let _gt = tokio::runtime::Handle::current().enter();
                tokio::task::spawn(async move {
                    let _ = commander.send(NeoReactorCommand::HangUp).await;
                    while set.join_next().await.is_some() {}
                    log::trace!("Dropped NeoReactor");
                });
            }
        }
    }
}

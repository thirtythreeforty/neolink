//! This is the highest level to a camera
//! it represents a collection of managed cameras
use anyhow::anyhow;
use std::collections::{hash_map::Entry, HashMap};
use tokio::sync::{
    mpsc::{channel as mpsc, Sender as MpscSender},
    oneshot::{channel as oneshot, Sender as OneshotSender},
};
use tokio_util::sync::CancellationToken;

use super::{NeoCam, NeoInstance};
use crate::{config::CameraConfig, Result};

#[allow(clippy::large_enum_variant)]
enum NeoReactorCommand {
    HangUp,
    Get(String, OneshotSender<Result<NeoInstance>>),
    GetOrInsert(CameraConfig, OneshotSender<Result<NeoInstance>>),
    UpdateOrInsert(CameraConfig, OneshotSender<Result<NeoInstance>>),
}

/// Reactor handles the collection of cameras
#[derive(Clone)]
pub(crate) struct NeoReactor {
    cancel: CancellationToken,
    commander: MpscSender<NeoReactorCommand>,
}

impl NeoReactor {
    pub(crate) async fn new() -> Self {
        let (commad_tx, mut command_rx) = mpsc(100);
        let me = Self {
            cancel: CancellationToken::new(),
            commander: commad_tx,
        };

        let cancel = me.cancel.clone();
        let cancel2 = me.cancel.clone();
        tokio::task::spawn(async move {
            let mut instances: HashMap<String, NeoCam> = Default::default();

            tokio::select! {
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
                            NeoReactorCommand::Get(name, sender) => {
                                let new = instances
                                    .get(&name)
                                    .ok_or_else(|| anyhow!("Camera not found"))
                                    .map(|data| data.subscribe())?
                                    .await;
                                let _ = sender.send(new);
                            }
                            NeoReactorCommand::GetOrInsert(config, sender) => {
                                let name = config.name.clone();
                                let new = match instances.entry(name) {
                                    Entry::Occupied(occ) => occ.get().subscribe().await,
                                    Entry::Vacant(vac) => {
                                        log::debug!("Inserting new insance");
                                        let cam = NeoCam::new(config).await?;
                                        log::debug!("New instance created");
                                        vac.insert(
                                            cam,
                                        )
                                        .subscribe()
                                        .await
                                    }
                                };
                                log::debug!("Got instance from reactor");
                                let _ = sender.send(new);
                            },
                            NeoReactorCommand::UpdateOrInsert(config, sender) => {
                                let name = config.name.clone();
                                let new = match instances.entry(name) {
                                    Entry::Occupied(occ) => {
                                        occ.get().update_config(config).await?;
                                        occ.get().subscribe().await
                                    },
                                    Entry::Vacant(vac) => {
                                        let cam = NeoCam::new(config).await?;
                                        vac.insert(cam)
                                        .subscribe()
                                        .await
                                    }
                                };
                                let _ = sender.send(new);
                            }
                        }
                    }
                    Ok(())
                } => v,
            }
        });

        me
    }

    #[allow(dead_code)]
    /// Get camera by name but do not create
    pub(crate) async fn get(&self, name: &str) -> Result<NeoInstance> {
        let (sender_tx, sender_rx) = oneshot();
        self.commander
            .send(NeoReactorCommand::Get(name.to_string(), sender_tx))
            .await?;

        sender_rx.await?
    }

    /// Get or create a camera
    pub(crate) async fn get_or_insert(&self, config: CameraConfig) -> Result<NeoInstance> {
        let (sender_tx, sender_rx) = oneshot();
        self.commander
            .send(NeoReactorCommand::GetOrInsert(config, sender_tx))
            .await?;

        sender_rx.await?
    }

    #[allow(dead_code)]
    /// Update a camera to a new config or create a camera
    pub(crate) async fn update_or_insert(&self, config: CameraConfig) -> Result<NeoInstance> {
        let (sender_tx, sender_rx) = oneshot();
        self.commander
            .send(NeoReactorCommand::UpdateOrInsert(config, sender_tx))
            .await?;

        sender_rx.await?
    }

    pub(crate) async fn shutdown(&self) {
        let _ = self.commander.send(NeoReactorCommand::HangUp).await;
        self.cancel.cancelled().await;
    }
}

impl Drop for NeoReactor {
    fn drop(&mut self) {
        log::debug!("Cancel:: NeoReactor::drop");
        self.cancel.cancel();
    }
}

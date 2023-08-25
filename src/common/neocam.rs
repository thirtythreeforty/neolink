//! This is the common code for creating a camera instance
//!
//! Features:
//!    Shared stream BC delivery
//!    Common restart code
//!    Clonable interface to share amongst threads
use crate::{config::CameraConfig, utils::connect_and_login, Result};
use neolink_core::bc_protocol::BcCamera;

use anyhow::anyhow;
use futures::stream::StreamExt;
use std::collections::{hash_map::Entry, HashMap};
use std::sync::{Arc, Weak};
use tokio::sync::{
    mpsc::{channel as mpsc, Sender as MpscSender, WeakSender as MpscWeakSender},
    oneshot::{channel as oneshot, Sender as OneshotSender},
    watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
};
use tokio::time::{interval, sleep, Duration, Instant};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

enum NeoCamCommand {
    HangUp,
    Instance(OneshotSender<Result<NeoInstance>>),
}
/// The underlying camera binding
struct NeoCam {
    config: WatchReceiver<CameraConfig>,
    cancel: CancellationToken,
    camera_watch: WatchSender<Weak<BcCamera>>,
    commander: ReceiverStream<NeoCamCommand>,
    sender: MpscWeakSender<NeoCamCommand>,
}

impl NeoCam {
    async fn init(config: CameraConfig) -> Result<(WatchSender<CameraConfig>, NeoInstance)> {
        let (watch_config_tx, watch_config_rx) = watch(config);
        let (camera_watch_tx, _) = watch(Weak::new());
        let (commander_tx, commander_rx) = mpsc(100);
        let mut me = Self {
            config: watch_config_rx,
            cancel: CancellationToken::new(),
            camera_watch: camera_watch_tx,
            commander: ReceiverStream::new(commander_rx),
            sender: commander_tx.downgrade(),
        };

        tokio::task::spawn(async move { me.run().await });

        let (instance_tx, instance_rx) = oneshot();
        commander_tx
            .send(NeoCamCommand::Instance(instance_tx))
            .await?;

        Ok((watch_config_tx, instance_rx.await??))
    }
    async fn run_camera(&mut self, config: &CameraConfig) -> Result<()> {
        let camera = Arc::new(connect_and_login(config).await?);

        self.camera_watch.send_replace(Arc::downgrade(&camera));

        let cancel = self.cancel.clone();
        let cancel_check = self.cancel.clone();
        // Now we wait for a disconnect
        tokio::select! {
            _ = cancel_check.cancelled() => {
                Ok(())
            }
            v = camera.join() => v,
            v = async {
                let mut interval = interval(Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    match camera.get_linktype().await {
                        Ok(_) => continue,
                        Err(neolink_core::Error::UnintelligibleReply { .. }) => {
                            // Camera does not support pings just wait forever
                            futures::pending!();
                        },
                        Err(e) => return Err(e),
                    }
                }
            } => v,
            v = async {
                while let Some(command) = self.commander.next().await {
                    match command {
                        NeoCamCommand::HangUp => {
                            cancel.cancel();
                            return Ok(());
                        }
                        NeoCamCommand::Instance(result) => {
                            let instance = NeoInstance::new(self);
                            let _ = result.send(instance);
                        }
                    }
                }
                Ok(())
            } => v
        }?;

        let _ = camera.logout().await;
        let _ = camera.shutdown().await;

        Ok(())
    }

    // Will run and attempt to maintain the connection
    // while also delivering messages
    async fn run(&mut self) -> Result<()> {
        const MAX_BACKOFF: Duration = Duration::from_secs(5);
        const MIN_BACKOFF: Duration = Duration::from_millis(50);

        let mut backoff = MIN_BACKOFF;

        loop {
            let mut config_rec = self.config.clone();

            let config = config_rec.borrow_and_update().clone();
            let now = Instant::now();

            let res = tokio::select! {
                Ok(_) = config_rec.changed() => {
                    None
                }
                v = self.run_camera(&config) => {
                    Some(v)
                }
            };
            self.camera_watch.send_replace(Weak::new());

            if res.is_none() {
                // If None go back and reload NOW
                //
                // This occurs if there was a config change
                continue;
            }

            // Else we see if the result actually was
            let result = res.unwrap();

            if Instant::now() - now > Duration::from_secs(60) {
                // Command ran long enough to be considered a success
                backoff = MIN_BACKOFF;
            }
            if backoff > MAX_BACKOFF {
                backoff = MAX_BACKOFF;
            }

            match result {
                Ok(()) => {
                    // Normal shutdown
                    self.cancel.cancel();
                    return Ok(());
                }
                Err(e) => {
                    // An error
                    // Check if it is non-retry
                    let e_inner = e.downcast_ref::<neolink_core::Error>();
                    match e_inner {
                        Some(neolink_core::Error::CameraLoginFail) => {
                            // Fatal
                            log::error!("Login credentials were not accepted");
                            self.cancel.cancel();
                            return Err(e);
                        }
                        _ => {
                            // Non fatal
                            log::warn!("Connection Lost: {:?}", e);
                            log::info!("Attempt reconnect in {:?}", backoff);
                            sleep(backoff).await;
                            backoff *= 2;
                        }
                    }
                }
            }
        }
    }
}

impl Drop for NeoCam {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// The sharable instance
///
/// This communicaes with the [`NeoCam`] over channels
///
/// The camera watch is used as an event to be triggered
/// whenever the camera is lost/updated
pub(crate) struct NeoInstance {
    camera_watch: tokio::sync::watch::Receiver<Weak<BcCamera>>,
    camera_control: MpscSender<NeoCamCommand>,
    cancel: CancellationToken,
}

impl NeoInstance {
    fn new(cam: &NeoCam) -> Result<Self> {
        Ok(Self {
            camera_watch: cam.camera_watch.subscribe(),
            camera_control: cam
                .sender
                .upgrade()
                .ok_or_else(|| anyhow!("Camera is shutting down"))?,
            cancel: cam.cancel.clone(),
        })
    }

    /// Create a new instance to the same camera
    pub(crate) async fn subscribe(&self) -> Result<Self> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::Instance(instance_tx))
            .await?;
        instance_rx.await?
    }

    pub(crate) async fn shutdown(&self) {
        let _ = self.camera_control.send(NeoCamCommand::HangUp).await;
        self.cancel.cancelled().await
    }

    /// This is a helpful convience function
    ///
    /// Given an async task it will:
    /// - Run the task with a reference to a BcCamera
    /// - If the camera instance is changed: Rerun the task with the new instance
    /// - If the camera returns a retryable error, wait for camera instance to change then rerun
    /// - else return the result of the function
    pub(crate) async fn run_task<F, T>(&self, task: F) -> Result<T>
    where
        F: for<'a> Fn(
            &'a BcCamera,
        ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<T>> + 'a>>,
    {
        let mut camera_watch = self.camera_watch.clone();
        let mut camera = camera_watch.borrow_and_update().upgrade();

        loop {
            let res = tokio::select! {
                _ = self.cancel.cancelled() => {
                    Some(Err(anyhow!("Camera is disconnecting")))
                }
                v = camera_watch.changed() => {
                    // Camera value has changed!
                    // update and try again
                    if v.is_ok() {
                        camera = camera_watch.borrow_and_update().upgrade();
                        None
                    } else {
                        Some(Err(anyhow!("Camera is disconnecting")))
                    }
                },
                Some(v) = async {
                    if let Some(cam) = camera.clone() {
                        let cam_ref = cam.as_ref();
                        Some(task(cam_ref).await)
                    } else {
                        None
                    }
                }, if camera.is_some() => {
                    match v {
                        // Ok means we are done
                        Ok(v) => Some(Ok(v)),
                        // If error we check for retryable errors
                        Err(e) => {
                            match e.downcast::<neolink_core::Error>() {
                                // Retry is a None
                                Ok(neolink_core::Error::DroppedConnection) => {
                                    camera = None;
                                    None
                                },
                                Ok(e) => Some(Err(e.into())),
                                Err(e) => Some(Err(e)),
                            }
                        }
                    }
                },
            };

            if let Some(res) = res {
                return res;
            }
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum NeoReactorCommand {
    HangUp,
    Get(String, OneshotSender<Result<NeoInstance>>),
    GetOrInsert(CameraConfig, OneshotSender<Result<NeoInstance>>),
    UpdateOrInsert(CameraConfig, OneshotSender<Result<NeoInstance>>),
}

struct NeoReactorData {
    instance: NeoInstance,
    config_sender: WatchSender<CameraConfig>,
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
            let mut instances: HashMap<String, NeoReactorData> = Default::default();

            tokio::select! {
                _ = cancel.cancelled() => {
                    for instance in instances.values() {
                        instance.instance.shutdown().await;
                    }
                    Ok(())
                },
                v = async {

                    while let Some(command) = command_rx.recv().await {
                        match command {
                            NeoReactorCommand::HangUp =>  {
                                for instance in instances.values() {
                                    instance.instance.shutdown().await;
                                }
                                cancel2.cancel();
                                return Result::<(), anyhow::Error>::Ok(());
                            }
                            NeoReactorCommand::Get(name, sender) => {
                                let new = instances
                                    .get(&name)
                                    .ok_or_else(|| anyhow!("Camera not found"))
                                    .map(|data| data.instance.subscribe())?
                                    .await;
                                let _ = sender.send(new);
                            }
                            NeoReactorCommand::GetOrInsert(config, sender) => {
                                let name = config.name.clone();
                                let new = match instances.entry(name) {
                                    Entry::Occupied(occ) => occ.get().instance.subscribe().await,
                                    Entry::Vacant(vac) => {
                                        let (config_sender, instance) = NeoCam::init(config).await?;
                                        vac.insert(NeoReactorData {
                                            instance,
                                            config_sender,
                                        })
                                        .instance
                                        .subscribe()
                                        .await
                                    }
                                };
                                let _ = sender.send(new);
                            },
                            NeoReactorCommand::UpdateOrInsert(config, sender) => {
                                let name = config.name.clone();
                                let new = match instances.entry(name) {
                                    Entry::Occupied(occ) => {
                                        occ.get().config_sender.send(config)?;
                                        occ.get().instance.subscribe().await
                                    },
                                    Entry::Vacant(vac) => {
                                        let (config_sender, instance) = NeoCam::init(config).await?;
                                        vac.insert(NeoReactorData {
                                            instance,
                                            config_sender,
                                        })
                                        .instance
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
        self.cancel.cancel();
    }
}

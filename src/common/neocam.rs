//! This is the common code for creating a camera instance
//!
//! Features:
//!    Shared stream BC delivery
//!    Common restart code
//!    Clonable interface to share amongst threads
use crate::{config::CameraConfig, utils::connect_and_login, Result};
use neolink_core::bc_protocol::{BcCamera, StreamKind};

use anyhow::anyhow;
use futures::stream::StreamExt;
use neolink_core::bcmedia::model::BcMedia;
use std::collections::{hash_map::Entry, HashMap};
use std::sync::{Arc, Weak};
use tokio::sync::{
    broadcast::{channel as broadcast, Sender as BroadcastSender},
    mpsc::{channel as mpsc, Receiver as MpscReceiver, Sender as MpscSender},
    oneshot::{channel as oneshot, Sender as OneshotSender},
    watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
};
use tokio::time::{interval, sleep, Duration, Instant};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_util::sync::CancellationToken;

enum NeoCamCommand {
    HangUp,
    Instance(OneshotSender<Result<NeoInstance>>),
    Stream(StreamKind, OneshotSender<BroadcastStream<BcMedia>>),
}
/// The underlying camera binding
struct NeoCam {
    cancel: CancellationToken,
    config_watch: WatchSender<CameraConfig>,
    commander: MpscSender<NeoCamCommand>,
    camera_watch: WatchReceiver<Weak<BcCamera>>,
}

impl NeoCam {
    async fn new(config: CameraConfig) -> Result<NeoCam> {
        let (commander_tx, commander_rx) = mpsc(100);
        let (watch_config_tx, watch_config_rx) = watch(config.clone());
        let (camera_watch_tx, camera_watch_rx) = watch(Weak::new());
        let (stream_request_tx, stream_request_rx) = mpsc(100);

        let me = Self {
            cancel: CancellationToken::new(),
            config_watch: watch_config_tx,
            commander: commander_tx.clone(),
            camera_watch: camera_watch_rx.clone(),
        };

        // This thread recieves messages from the instances
        // and acts on it.
        //
        // This thread must be sta rted first so that we can begin creating instances for the
        // other threads
        let sender_cancel = me.cancel.clone();
        let mut commander_rx = ReceiverStream::new(commander_rx);
        let strict = config.strict;
        let thread_commander_tx = commander_tx.clone();
        tokio::task::spawn(async move {
            let thread_cancel = sender_cancel.clone();
            let res = tokio::select! {
                _ = sender_cancel.cancelled() => Result::Ok(()),
                v = async {
                    while let Some(command) = commander_rx.next().await {
                        match command {
                            NeoCamCommand::HangUp => {
                                sender_cancel.cancel();
                                log::debug!("Cancel:: NeoCamCommand::HangUp");
                                return Result::<(), anyhow::Error>::Ok(());
                            }
                            NeoCamCommand::Instance(result) => {
                                let instance = NeoInstance::new(
                                    camera_watch_rx.clone(),
                                    thread_commander_tx.clone(),
                                    thread_cancel.clone(),
                                );
                                let _ = result.send(instance);
                            }
                            NeoCamCommand::Stream(name, sender) => {
                                stream_request_tx.send(
                                    StreamRequest {
                                        name,
                                        sender,
                                        strict,
                                    }
                                ).await?;
                            },
                        }
                    }
                    Ok(())
                } => v
            };
            log::debug!("Control thread terminated");
            res
        });

        let mut cam_thread =
            NeoCamThread::new(watch_config_rx, camera_watch_tx, me.cancel.clone()).await;

        // This thread maintains the camera loop
        //
        // It will keep it logged and reconnect
        tokio::task::spawn(async move { cam_thread.run().await });

        let (instance_tx, instance_rx) = oneshot();
        commander_tx
            .send(NeoCamCommand::Instance(instance_tx))
            .await?;

        let instance = instance_rx.await??;

        // This thread maintains the streams
        let stream_instance = instance.subscribe().await?;
        let stream_cancel = me.cancel.clone();
        let mut stream_thread =
            NeoCamStreamThread::new(stream_request_rx, stream_instance, stream_cancel).await;

        tokio::task::spawn(async move { stream_thread.run().await });

        Ok(me)
    }

    async fn subscribe(&self) -> Result<NeoInstance> {
        NeoInstance::new(
            self.camera_watch.clone(),
            self.commander.clone(),
            self.cancel.clone(),
        )
    }

    pub(crate) async fn shutdown(&self) {
        let _ = self.commander.send(NeoCamCommand::HangUp).await;
        self.cancel.cancelled().await
    }
}

impl Drop for NeoCam {
    fn drop(&mut self) {
        log::debug!("Cancel:: NeoCam::drop");
        self.cancel.cancel();
    }
}

struct NeoCamThread {
    config: WatchReceiver<CameraConfig>,
    cancel: CancellationToken,
    camera_watch: WatchSender<Weak<BcCamera>>,
}

impl NeoCamThread {
    async fn new(
        watch_config_rx: WatchReceiver<CameraConfig>,
        camera_watch_tx: WatchSender<Weak<BcCamera>>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            config: watch_config_rx,
            cancel,
            camera_watch: camera_watch_tx,
        }
    }
    async fn run_camera(&mut self, config: &CameraConfig) -> Result<()> {
        let camera = Arc::new(connect_and_login(config).await?);

        self.camera_watch.send_replace(Arc::downgrade(&camera));

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
                    log::debug!("Cancel:: NeoCamThread::NormalShutdown");
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

impl Drop for NeoCamThread {
    fn drop(&mut self) {
        log::debug!("Cancel:: NeoCamThread::drop");
        self.cancel.cancel();
    }
}

struct StreamRequest {
    name: StreamKind,
    sender: OneshotSender<BroadcastStream<BcMedia>>,
    strict: bool,
}

struct StreamData {
    sender: BroadcastSender<BcMedia>,
    name: StreamKind,
    instance: NeoInstance,
    strict: bool,
    cancel: CancellationToken,
}

impl StreamData {
    async fn run(&self) -> Result<()> {
        let thread_stream_tx = self.sender.clone();
        let cancel = self.cancel.clone();
        let instance = self.instance.subscribe().await?;
        let name = self.name;
        let strict = self.strict;
        tokio::task::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {
                    Result::<(), anyhow::Error>::Ok(())
                },
                v = async {
                    loop {
                        instance.run_task(|camera| {
                            let stream_tx = thread_stream_tx.clone();
                            Box::pin(async move {
                                let mut stream_data = camera.start_video(name, 0, strict).await?;
                                loop {
                                    let data = stream_data.get_data().await??;
                                    stream_tx.send(data)?;
                                }
                            })
                        }).await?;
                    }
                }    => v,
            }
        });

        Ok(())
    }
}

impl Drop for StreamData {
    fn drop(&mut self) {
        log::debug!("Cancel:: StreamData::drop");
        self.cancel.cancel();
    }
}

/// This thread will start and stop the camera
/// based on the number of listeners to the
/// async streams
struct NeoCamStreamThread {
    streams: HashMap<StreamKind, StreamData>,
    stream_request_rx: MpscReceiver<StreamRequest>,
    cancel: CancellationToken,
    instance: NeoInstance,
}

impl NeoCamStreamThread {
    async fn new(
        stream_request_rx: MpscReceiver<StreamRequest>,
        instance: NeoInstance,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            streams: Default::default(),
            stream_request_rx,
            cancel,
            instance,
        }
    }
    async fn run(&mut self) -> Result<()> {
        let thread_cancel = self.cancel.clone();
        tokio::select! {
            _ = thread_cancel.cancelled() => Ok(()),
            v = async {
                while let Some(request) = self.stream_request_rx.recv().await {
                    match self.streams.entry(request.name) {
                        Entry::Occupied(occ) => {
                            let _ = request
                                .sender
                                .send(BroadcastStream::new(occ.get().sender.subscribe()));
                        }
                        Entry::Vacant(vac) => {
                            // Make a new streaming instance

                            let (sender, stream_rx) = broadcast(1000);
                            let data = StreamData {
                                sender,
                                name: request.name,
                                instance: self.instance.subscribe().await?,
                                strict: request.strict,
                                cancel: CancellationToken::new(),
                            };
                            data.run().await?;
                            vac.insert(data);
                            let _ = request.sender.send(BroadcastStream::new(stream_rx));
                        }
                    }
                }
                Ok(())
            } => v,
        }
    }
}

/// The sharable instance
///
/// This communicaes with the [`NeoCam`] over channels
///
/// The camera watch is used as an event to be triggered
/// whenever the camera is lost/updated
pub(crate) struct NeoInstance {
    camera_watch: WatchReceiver<Weak<BcCamera>>,
    camera_control: MpscSender<NeoCamCommand>,
    cancel: CancellationToken,
}

impl NeoInstance {
    fn new(
        camera_watch: WatchReceiver<Weak<BcCamera>>,
        camera_control: MpscSender<NeoCamCommand>,
        cancel: CancellationToken,
    ) -> Result<Self> {
        Ok(Self {
            camera_watch,
            camera_control,
            cancel,
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
        )
            -> std::pin::Pin<Box<dyn futures::Future<Output = Result<T>> + Send + 'a>>,
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

    pub(crate) async fn stream(&self, name: StreamKind) -> Result<BroadcastStream<BcMedia>> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::Stream(name, instance_tx))
            .await?;
        Ok(instance_rx.await?)
    }
}

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
                                        occ.get().config_watch.send(config)?;
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

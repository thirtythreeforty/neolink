//! This thread will start and stop the camera
//!
//! If there are no listeners to the broadcast
//! then it will hangup

use futures::stream::{FuturesUnordered, StreamExt};
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};
use tokio::{
    sync::{
        broadcast::{
            channel as broadcast, Receiver as BroadcastReceiver, Sender as BroadcastSender,
        },
        mpsc::{channel as mpsc, Receiver as MpscReceiver, Sender as MpscSender},
        oneshot::Sender as OneshotSender,
        watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
    },
    task::JoinHandle,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

use super::NeoInstance;
use crate::{AnyResult, Result};
use neolink_core::{bc_protocol::StreamKind, bcmedia::model::*};

pub(crate) struct NeoCamStreamThread {
    streams: HashMap<StreamKind, StreamData>,
    stream_request_rx: MpscReceiver<StreamRequest>,
    cancel: CancellationToken,
    instance: NeoInstance,
}

impl NeoCamStreamThread {
    pub(crate) async fn new(
        stream_request_rx: MpscReceiver<StreamRequest>,
        instance: NeoInstance,
        cancel: CancellationToken,
    ) -> Result<Self> {
        Ok(Self {
            streams: Default::default(),
            stream_request_rx,
            cancel,
            instance,
        })
    }
    pub(crate) async fn run(&mut self) -> Result<()> {
        let thread_cancel = self.cancel.clone();
        tokio::select! {
            _ = thread_cancel.cancelled() => {
                for (_, mut data) in self.streams.drain() {
                    let _ = data.shutdown().await;
                }
                Ok(())
            },
            v = async {
                while let Some(request) = self.stream_request_rx.recv().await {
                    match request {
                        StreamRequest::Get {
                            name, sender
                        } => {
                          if let Entry::Occupied(occ) = self.streams.entry(name) {
                            let _ = sender.send(Some(
                                StreamInstance::new(occ.get()).await?));
                          } else {
                              let _ = sender.send(None);
                          }
                        },
                        StreamRequest::GetOrInsert {
                            name, sender, strict
                        } => {
                            match self.streams.entry(name) {
                                Entry::Occupied(occ) => {
                                    let _ = sender
                                        .send(StreamInstance::new(occ.get()).await?);
                                }
                                Entry::Vacant(vac) => {
                                    // Make a new streaming instance

                                    let data = StreamData::new(
                                        name,
                                        self.instance.subscribe().await?,
                                        strict,
                                    ).await?;
                                    let config = data.config.subscribe();
                                    let vid = data.vid.subscribe();
                                    let aud = data.aud.subscribe();
                                    let in_use = data.users.activated().await?;

                                    let data = vac.insert(data);

                                    let _ = sender.send(StreamInstance::new(data).await?);
                                }
                            }
                        },
                        StreamRequest::High {
                            sender
                        } => {
                            let mut result = None;
                            let mut streams = vec![
                                StreamKind::Main,
                                StreamKind::Extern,
                                StreamKind::Sub,
                            ];
                            for name in streams.drain(..) {
                                if let Entry::Occupied(occ) = self.streams.entry(name) {
                                        result = Some(
                                            StreamInstance::new(occ.get()).await?);
                                        break;
                                }
                            }
                            let _ = sender.send(result);
                        },
                        StreamRequest::Low {
                            sender
                        } => {
                            let mut result = None;
                            let mut streams = vec![
                                StreamKind::Sub,
                                StreamKind::Extern,
                                StreamKind::Main,
                            ];
                            for name in streams.drain(..) {
                                if let Entry::Occupied(occ) = self.streams.entry(name) {
                                        result = Some(
                                            StreamInstance {
                                                name,
                                                vid: occ.get().vid.subscribe(),
                                                aud: occ.get().aud.subscribe(),
                                                config: occ.get().config.subscribe(),
                                                in_use: occ.get().users.activated().await?,
                                            });
                                        break;
                                }
                            }
                            let _ = sender.send(result);
                        },
                        StreamRequest::All {
                            sender
                        } => {
                            let config = self.instance.config().await?.borrow_and_update().clone();
                            let streams = config.stream.as_stream_kinds();
                            for stream in streams.iter().copied() {
                                if let Entry::Vacant(vac) = self.streams.entry(stream) {
                                    vac.insert(
                                        StreamData::new(stream, self.instance.subscribe().await?, config.strict)
                                            .await?,
                                    );
                                }
                            }
                            let mut streams = self.streams.iter_mut().filter_map(|(name, stream)| if streams.contains(name) {
                                    Some(async move {
                                        Result::<_, anyhow::Error>::Ok(
                                            StreamInstance::new(&stream).await?
                                        )
                                    })
                                } else {
                                    None
                                }
                            ).collect::<FuturesUnordered<_>>().collect::<Vec<_>>().await;
                            let _ = sender.send(streams.drain(..).flatten().collect());
                        }
                    }
                }
                Ok(())
            } => v,
        }
    }
}

impl Drop for NeoCamStreamThread {
    fn drop(&mut self) {
        self.cancel.cancel();
        for stream in self.streams.values() {
            stream.cancel.cancel()
        }
    }
}

/// The kind of stream we want a async broadcast of
pub(crate) enum StreamRequest {
    #[allow(dead_code)]
    /// Get a currently loaded stream
    Get {
        name: StreamKind,
        sender: OneshotSender<Option<StreamInstance>>,
    },
    /// Get or Insert a stream
    GetOrInsert {
        name: StreamKind,
        sender: OneshotSender<StreamInstance>,
        strict: bool,
    },
    /// Get highest available stream. Which this is depends on what is
    /// disabled
    High {
        sender: OneshotSender<Option<StreamInstance>>,
    },
    /// Get lowest quality available stream. Which this is depends on what is
    /// disabled
    Low {
        sender: OneshotSender<Option<StreamInstance>>,
    },
    /// Get all streams configured in the config
    All {
        sender: OneshotSender<Vec<StreamInstance>>,
    },
}

/// Counts the active users of the stream
pub(crate) struct UseCounter {
    value: WatchReceiver<u32>,
    notifier_tx: MpscSender<bool>,
    cancel: CancellationToken,
}

impl UseCounter {
    async fn new() -> Self {
        let (notifier_tx, mut notifier) = mpsc(100);
        let (value_tx, value) = watch(0);
        let cancel = CancellationToken::new();

        let thread_cancel = cancel.clone();
        tokio::task::spawn(async move {
            tokio::select! {
                _ = thread_cancel.cancelled() => {
                    AnyResult::Ok(())
                },
                v = async {
                    while let Some(noti) = notifier.recv().await {
                        value_tx.send_modify(|value| {
                            if noti {
                                *value += 1;
                            } else {
                                *value -= 1;
                            }
                        });
                    }
                    AnyResult::Ok(())
                } => v,
            }
        });
        Self {
            value,
            notifier_tx,
            cancel,
        }
    }

    async fn activated(&self) -> Result<CountUses> {
        let mut res = CountUses::new(self);
        res.activate().await?;
        Ok(res)
    }

    async fn deactivated(&self) -> Result<CountUses> {
        Ok(CountUses::new(self))
    }
}

impl Drop for UseCounter {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub(crate) struct CountUses {
    is_active: bool,
    value: WatchReceiver<u32>,
    notifier: MpscSender<bool>,
}

impl CountUses {
    fn new(source: &UseCounter) -> Self {
        Self {
            is_active: false,
            value: source.value.clone(),
            notifier: source.notifier_tx.clone(),
        }
    }

    pub(crate) async fn activate(&mut self) -> Result<()> {
        if !self.is_active {
            self.is_active = true;
            self.notifier.send(self.is_active).await?;
        }
        Ok(())
    }

    pub(crate) async fn deactivate(&mut self) -> Result<()> {
        if self.is_active {
            self.is_active = false;
            self.notifier.send(self.is_active).await?;
        }
        Ok(())
    }

    pub(crate) async fn aquired_users(&self) -> Result<()> {
        self.value.clone().wait_for(|curr| *curr > 0).await?;
        Ok(())
    }

    pub(crate) async fn dropped_users(&self) -> Result<()> {
        self.value.clone().wait_for(|curr| *curr == 0).await?;
        Ok(())
    }
}

impl Drop for CountUses {
    fn drop(&mut self) {
        if self.is_active {
            self.is_active = false;
            tokio::task::block_in_place(move || {
                tokio::runtime::Handle::current().block_on(async move {
                    let _ = self.notifier.send(self.is_active).await;
                });
            });
        }
    }
}

/// The data of a running stream
pub(crate) struct StreamData {
    vid: BroadcastSender<StampedData>,
    aud: BroadcastSender<StampedData>,
    config: Arc<WatchSender<StreamConfig>>,
    name: StreamKind,
    instance: NeoInstance,
    cancel: CancellationToken,
    handle: Option<JoinHandle<Result<()>>>,
    strict: bool,
    users: UseCounter,
}

#[derive(Eq, PartialEq, Clone)]
pub(crate) enum VidFormat {
    None,
    H264,
    H265,
}
#[derive(Eq, PartialEq, Clone)]
pub(crate) enum AudFormat {
    None,
    Aac,
    Adpcm(u32),
}

#[derive(Clone)]
pub(crate) struct StreamConfig {
    pub(crate) resolution: [u32; 2],
    pub(crate) vid_format: VidFormat,
    pub(crate) aud_format: AudFormat,
}

#[derive(Debug, Clone)]
pub(crate) struct StampedData {
    pub(crate) data: Vec<u8>,
    pub(crate) ts: Duration,
}

pub(crate) struct StreamInstance {
    pub(crate) name: StreamKind,
    pub(crate) vid: BroadcastReceiver<StampedData>,
    pub(crate) aud: BroadcastReceiver<StampedData>,
    pub(crate) config: WatchReceiver<StreamConfig>,
    in_use: CountUses,
}

impl StreamInstance {
    pub async fn new(data: &StreamData) -> Result<Self> {
        Ok(Self {
            name: data.name,
            vid: data.vid.subscribe(),
            aud: data.aud.subscribe(),
            config: data.config.subscribe(),
            in_use: data.users.activated().await?,
        })
    }
    pub(crate) async fn activate(&mut self) -> Result<()> {
        self.in_use.activate().await
    }
    pub(crate) async fn deactivate(&mut self) -> Result<()> {
        self.in_use.activate().await
    }
}

impl StreamData {
    async fn new(name: StreamKind, instance: NeoInstance, strict: bool) -> Result<Self> {
        let (vid, _) = broadcast::<StampedData>(100);
        let (aud, _) = broadcast::<StampedData>(100);
        let resolution = instance
            .run_task(|cam| {
                Box::pin(async move {
                    let infos = cam
                        .get_stream_info()
                        .await?
                        .stream_infos
                        .iter()
                        .flat_map(|info| info.encode_tables.clone())
                        .collect::<Vec<_>>();
                    if let Some(encode) =
                        infos.iter().find(|encode| encode.name == name.to_string())
                    {
                        Ok([encode.resolution.width, encode.resolution.height])
                    } else {
                        Ok([0, 0])
                    }
                })
            })
            .await?;
        let (config_tx, _) = watch(StreamConfig {
            resolution,
            vid_format: VidFormat::None,
            aud_format: AudFormat::None,
        });
        let mut me = Self {
            name,
            cancel: CancellationToken::new(),
            config: Arc::new(config_tx),
            vid,
            aud,
            instance,
            handle: None,
            strict,
            users: UseCounter::new().await,
        };

        let cancel = me.cancel.clone();
        let vid = me.vid.clone();
        let aud = me.aud.clone();
        let instance = me.instance.subscribe().await?;
        let name = me.name;
        let strict = me.strict;
        let config = me.config.clone();
        let thread_inuse = me.users.deactivated().await?;

        me.handle = Some(tokio::task::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {
                    Result::<(), anyhow::Error>::Ok(())
                },
                v = async {
                    loop {
                        tokio::select! {
                            v = thread_inuse.dropped_users() => {
                                // Handles the stop and restart when no active users
                                v?;
                                thread_inuse.aquired_users().await?; // Wait for new users of the stream
                                AnyResult::Ok(())
                            },
                            result = instance.run_task(|camera| {
                                    let vid_tx = vid.clone();
                                    let aud_tx = aud.clone();
                                    let stream_config = config.clone();
                                    Box::pin(async move {
                                        let res = async {
                                            let mut prev_ts = Duration::ZERO;
                                            let mut stream_data = camera.start_video(name, 0, strict).await?;
                                            loop {
                                                let data = stream_data.get_data().await??;

                                                // Update the stream config with any information
                                                match &data {
                                                    BcMedia::InfoV1(info) => {
                                                        stream_config.send_if_modified(|state| {
                                                            if state.resolution[0] != info.video_width || state.resolution[1] != info.video_height {
                                                                state.resolution[0] = info.video_width;
                                                                state.resolution[1] = info.video_height;
                                                                true
                                                            } else {
                                                                false
                                                            }
                                                        });
                                                    },
                                                    BcMedia::InfoV2(info) => {
                                                        stream_config.send_if_modified(|state| {
                                                            if state.resolution[0] != info.video_width || state.resolution[1] != info.video_height {
                                                                state.resolution[0] = info.video_width;
                                                                state.resolution[1] = info.video_height;
                                                                true
                                                            } else {
                                                                false
                                                            }
                                                        });
                                                    },
                                                    BcMedia::Iframe(frame) => {
                                                        stream_config.send_if_modified(|state| {
                                                            let expected = match frame.video_type {
                                                                VideoType::H264 => VidFormat::H264,
                                                                VideoType::H265 => VidFormat::H265,
                                                            };
                                                            if state.vid_format != expected {
                                                                state.vid_format = expected;
                                                                true
                                                            } else {
                                                                false
                                                            }
                                                        });
                                                    }
                                                    BcMedia::Pframe(frame) => {
                                                        stream_config.send_if_modified(|state| {
                                                            let expected = match frame.video_type {
                                                                VideoType::H264 => VidFormat::H264,
                                                                VideoType::H265 => VidFormat::H265,
                                                            };
                                                            if state.vid_format != expected {
                                                                state.vid_format = expected;
                                                                true
                                                            } else {
                                                                false
                                                            }
                                                        });
                                                    },
                                                    BcMedia::Aac(_) => {
                                                        stream_config.send_if_modified(|state| {
                                                            if state.aud_format != AudFormat::Aac {
                                                                state.aud_format = AudFormat::Aac;
                                                                true
                                                            } else {
                                                                false
                                                            }
                                                        });
                                                    }
                                                    BcMedia::Adpcm(aud) => {
                                                        stream_config.send_if_modified(|state| {
                                                            let expected = AudFormat::Adpcm(aud.data.len() as u32 - 4);
                                                            if state.aud_format != expected {
                                                                state.aud_format = expected;
                                                                true
                                                            } else {
                                                                false
                                                            }
                                                        });
                                                    }
                                                }

                                                match data {
                                                    BcMedia::Iframe(BcMediaIframe{data, microseconds, ..}) | BcMedia::Pframe(BcMediaPframe{data, microseconds,..}) => {
                                                        prev_ts = Duration::from_micros(microseconds as u64);
                                                        let _ = vid_tx.send(
                                                            StampedData{
                                                                data,
                                                                ts: prev_ts
                                                        });
                                                        log::trace!("Sent Vid Frame");
                                                    }
                                                    BcMedia::Aac(BcMediaAac{data, ..}) | BcMedia::Adpcm(BcMediaAdpcm{data,..}) => {
                                                        let _ = aud_tx.send(
                                                            StampedData{
                                                                data,
                                                                ts: prev_ts,
                                                        })?;
                                                        log::trace!("Sent Aud Frame");
                                                    },
                                                    _ => {},
                                                }
                                            }
                                            Result::<(),anyhow::Error>::Ok(())
                                        }.await;
                                        Ok(res)
                                    })
                                }) => {
                                match result {
                                    Ok(Ok(())) => {
                                        log::debug!("Video Stream Stopped due to no listeners");
                                        break Ok(());
                                    },
                                    Ok(Err(e)) => {
                                        log::debug!("Video Stream Restarting Due to Error: {:?}", e);
                                        AnyResult::Ok(())
                                    },
                                    Err(e) => {
                                        log::debug!("Video Stream Stopped Due to Instance Error: {:?}", e);
                                        break Err(e);
                                    },
                                }
                            },
                        }?;
                    }
                }    => v,
            }
        }));

        Ok(me)
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.cancel.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        Ok(())
    }
}

impl Drop for StreamData {
    fn drop(&mut self) {
        log::debug!("Cancel:: StreamData::drop");
        self.cancel.cancel();
    }
}

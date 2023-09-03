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
        mpsc::Receiver as MpscReceiver,
        oneshot::Sender as OneshotSender,
        watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
    },
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use super::NeoInstance;
use crate::Result;
use neolink_core::{
    bc_protocol::StreamKind,
    bcmedia::model::{BcMedia, VideoType},
};

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
            _ = thread_cancel.cancelled() => Ok(()),
            v = async {
                while let Some(request) = self.stream_request_rx.recv().await {
                    match request {
                        StreamRequest::Get {
                            name, sender
                        } => {
                          if let Entry::Occupied(mut occ) = self.streams.entry(name) {
                              let sub = occ.get().sender.subscribe();
                                occ.get_mut().ensure_running().await?;

                                let _ = sender.send(Some(
                                    StreamInstance {
                                        name,
                                        stream: sub,
                                        config: occ.get().config.subscribe(),
                                    }));
                          } else {
                              let _ = sender.send(None);
                          }
                        },
                        StreamRequest::GetOrInsert {
                            name, sender, strict
                        } => {
                            match self.streams.entry(name) {
                                Entry::Occupied(mut occ) => {
                                    let sub = occ.get().sender.subscribe();
                                    occ.get_mut().ensure_running().await?;
                                    let _ = sender
                                        .send(StreamInstance {
                                            name,
                                            stream: sub,
                                            config: occ.get().config.subscribe(),
                                        });
                                }
                                Entry::Vacant(vac) => {
                                    // Make a new streaming instance

                                    let (sender_tx, stream_rx) = broadcast(1000);
                                    let mut data = StreamData::new(
                                        sender_tx,
                                        name,
                                        self.instance.subscribe().await?,
                                        strict,
                                    ).await?;
                                    data.ensure_running().await?;
                                    let config = data.config.subscribe();
                                    vac.insert(data);
                                    let _ = sender.send(StreamInstance {
                                        name,
                                        stream: stream_rx,
                                        config,
                                    });
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
                                if let Entry::Occupied(mut occ) = self.streams.entry(name) {
                                    let sub = occ.get().sender.subscribe();
                                        occ.get_mut().ensure_running().await?;

                                        result = Some(
                                            StreamInstance {
                                                name,
                                                stream: sub,
                                                config: occ.get().config.subscribe(),
                                            });
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
                                if let Entry::Occupied(mut occ) = self.streams.entry(name) {
                                    let sub = occ.get().sender.subscribe();
                                        occ.get_mut().ensure_running().await?;

                                        result = Some(
                                            StreamInstance {
                                                name,
                                                stream: sub,
                                                config: occ.get().config.subscribe(),
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
                                    let (tx, _) = broadcast(1000);
                                    vac.insert(
                                        StreamData::new(tx, stream, self.instance.subscribe().await?, config.strict)
                                            .await?,
                                    );
                                }
                            }
                            let mut streams = self.streams.iter_mut().filter_map(|(name, stream)| if streams.contains(name) {
                                    Some(async move {
                                        let sub = stream.sender.subscribe();
                                        stream.ensure_running().await?;
                                        Result::<_, anyhow::Error>::Ok(
                                            StreamInstance {
                                                name: *name,
                                                stream: sub,
                                                config: stream.config.subscribe(),
                                            })
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

/// The data of a running stream
pub(crate) struct StreamData {
    sender: BroadcastSender<BcMedia>,
    config: Arc<WatchSender<StreamConfig>>,
    name: StreamKind,
    instance: NeoInstance,
    cancel: CancellationToken,
    handle: Option<JoinHandle<Result<()>>>,
    strict: bool,
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

pub(crate) struct StreamInstance {
    pub(crate) name: StreamKind,
    pub(crate) stream: BroadcastReceiver<BcMedia>,
    pub(crate) config: WatchReceiver<StreamConfig>,
}

impl StreamData {
    async fn new(
        sender: BroadcastSender<BcMedia>,
        name: StreamKind,
        instance: NeoInstance,
        strict: bool,
    ) -> Result<Self> {
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
        let me = Self {
            name,
            cancel: CancellationToken::new(),
            config: Arc::new(config_tx),
            sender,
            instance,
            handle: None,
            strict,
        };
        Ok(me)
    }

    async fn ensure_running(&mut self) -> Result<()> {
        if self.cancel.is_cancelled()
            || self
                .handle
                .as_ref()
                .map(|handle| handle.is_finished())
                .unwrap_or(true)
        {
            log::debug!("Restart stream");
            self.restart().await?;
        }
        Ok(())
    }

    async fn restart(&mut self) -> Result<()> {
        self.shutdown().await?;
        self.cancel = CancellationToken::new();

        let cancel = self.cancel.clone();
        let sender = self.sender.clone();
        let instance = self.instance.subscribe().await?;
        let name = self.name;
        let strict = self.strict;
        let config = self.config.clone();
        self.handle = Some(tokio::task::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {
                    Result::<(), anyhow::Error>::Ok(())
                },
                v = async {
                    loop {
                        let result = instance.run_task(|camera| {
                            let stream_tx = sender.clone();
                            let stream_config = config.clone();
                            Box::pin(async move {
                                let res = async {
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

                                        // Send the stream data onwards
                                        if stream_tx.send(data).is_err() {
                                            // If noone is listening for the stream we error and stop here
                                            break;
                                        };
                                    }
                                    Result::<(),anyhow::Error>::Ok(())
                                }.await;
                                Ok(res)
                            })
                        }).await;
                        match result {
                            Ok(Ok(())) => {
                                log::debug!("Video Stream Stopped due to no listeners");
                                break;
                            },
                            Ok(Err(e)) => {
                                log::debug!("Video Stream Restarting Due to Error: {:?}", e);
                            },
                            Err(e) => {
                                log::debug!("Video Stream Stopped Due to Instance Error: {:?}", e);
                                break;
                            },
                        }
                    }
                    Ok(())
                }    => v,
            }
        }));

        Ok(())
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

//! This thread will start and stop the camera
//!
//! If there are no listeners to the broadcast
//! then it will hangup

use futures::stream::{FuturesUnordered, StreamExt};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    sync::Arc,
};
use tokio::{
    sync::{
        broadcast::{
            channel as broadcast, Receiver as BroadcastReceiver, Sender as BroadcastSender,
        },
        mpsc::{channel as mpsc, Receiver as MpscReceiver},
        oneshot::Sender as OneshotSender,
        watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
    },
    task::JoinHandle,
    time::{timeout, Duration},
};
use tokio_util::sync::CancellationToken;

use super::{NeoInstance, Permit, UseCounter};
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
    ) -> Result<Self> {
        Ok(Self {
            streams: Default::default(),
            stream_request_rx,
            cancel: CancellationToken::new(),
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
                            let config = self.instance.config().await?.borrow().clone();
                            let config_streams = config.stream.as_stream_kinds();
                            for name in streams.drain(..) {
                                if config_streams.contains(&name) {
                                    // Fill it in
                                    if let Entry::Vacant(vac) = self.streams.entry(name) {
                                        vac.insert(
                                            StreamData::new(name, self.instance.subscribe().await?, config.strict)
                                                .await?,
                                        );
                                    }

                                    // Grab it
                                    if let Entry::Occupied(occ) = self.streams.entry(name) {
                                            result = Some(
                                                StreamInstance::new(occ.get()).await?);
                                            break;
                                    }
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
                            let config = self.instance.config().await?.borrow().clone();
                            let config_streams = config.stream.as_stream_kinds();
                            for name in streams.drain(..) {
                                if config_streams.contains(&name) {
                                    // Fill it in
                                    if let Entry::Vacant(vac) = self.streams.entry(name) {
                                        vac.insert(
                                            StreamData::new(name, self.instance.subscribe().await?, config.strict)
                                                .await?,
                                        );
                                    }

                                    // Grab it
                                    if let Entry::Occupied(occ) = self.streams.entry(name) {
                                            result = Some(
                                                StreamInstance::new(occ.get()).await?
                                            );
                                            break;
                                    }
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
                                        StreamInstance::new(stream).await
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
        log::debug!("NeoCamStreamThread::drop Cancel");
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
    vid: BroadcastSender<StampedData>,
    aud: BroadcastSender<StampedData>,
    vid_history: Arc<WatchSender<VecDeque<StampedData>>>,
    aud_history: Arc<WatchSender<VecDeque<StampedData>>>,
    config: Arc<WatchSender<StreamConfig>>,
    name: StreamKind,
    instance: NeoInstance,
    cancel: CancellationToken,
    handle: Option<JoinHandle<Result<()>>>,
    strict: bool,
    users: UseCounter,
}

#[derive(Eq, PartialEq, Clone, Debug)]
pub(crate) enum VidFormat {
    None,
    H264,
    H265,
}
#[derive(Eq, PartialEq, Clone, Debug)]
pub(crate) enum AudFormat {
    None,
    Aac,
    Adpcm(u32),
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) struct StreamConfig {
    pub(crate) resolution: [u32; 2],
    pub(crate) vid_format: VidFormat,
    pub(crate) aud_format: AudFormat,
}

#[derive(Debug, Clone)]
pub(crate) struct StampedData {
    pub(crate) keyframe: bool,
    pub(crate) data: Arc<Vec<u8>>,
    pub(crate) ts: Duration,
}

pub(crate) struct StreamInstance {
    #[allow(dead_code)]
    pub(crate) name: StreamKind,
    pub(crate) vid: BroadcastReceiver<StampedData>,
    pub(crate) vid_history: WatchReceiver<VecDeque<StampedData>>,
    pub(crate) aud: BroadcastReceiver<StampedData>,
    pub(crate) aud_history: WatchReceiver<VecDeque<StampedData>>,
    pub(crate) config: WatchReceiver<StreamConfig>,
    in_use: Permit,
}

impl StreamInstance {
    pub async fn new(data: &StreamData) -> Result<Self> {
        Ok(Self {
            name: data.name,
            vid: data.vid.subscribe(),
            vid_history: data.vid_history.subscribe(),
            aud: data.aud.subscribe(),
            aud_history: data.aud_history.subscribe(),
            config: data.config.subscribe(),
            in_use: data.users.create_activated().await?,
        })
    }
    pub(crate) async fn activate(&mut self) -> Result<()> {
        self.in_use.activate().await
    }
    pub(crate) async fn deactivate(&mut self) -> Result<()> {
        self.in_use.deactivate().await
    }

    pub(crate) async fn activator_handle(&mut self) -> Permit {
        self.in_use.subscribe()
    }
}

impl StreamData {
    async fn new(name: StreamKind, instance: NeoInstance, strict: bool) -> Result<Self> {
        let (vid, _) = broadcast::<StampedData>(100);
        let (aud, _) = broadcast::<StampedData>(100);
        let (vid_history, _) = watch::<VecDeque<StampedData>>(VecDeque::new());
        let vid_history = Arc::new(vid_history);
        let (aud_history, _) = watch::<VecDeque<StampedData>>(VecDeque::new());
        let aud_history = Arc::new(aud_history);
        let resolution = instance
            .run_passive_task(|cam| {
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
            vid_history,
            aud,
            aud_history,
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
        let thread_inuse = me.users.create_deactivated().await?;
        let vid_history = me.vid_history.clone();
        let aud_history = me.aud_history.clone();

        me.handle = Some(tokio::task::spawn(async move {
            let r = tokio::select! {
                _ = cancel.cancelled() => {
                    Result::<(), anyhow::Error>::Ok(())
                },
                v = async {
                    loop {
                        let (watchdog_tx, mut watchdog_rx) = mpsc(1);
                        tokio::select! {
                            v = thread_inuse.dropped_users() => {
                                // Handles the stop and restart when no active users
                                log::debug!("Streaming STOP");
                                v?;
                                thread_inuse.aquired_users().await?; // Wait for new users of the stream
                                log::debug!("Streaming START");
                                AnyResult::Ok(())
                            },
                            v = async {
                                watchdog_rx.recv().await; // Wait forever for the first feed
                                loop {
                                    let check_timeout = timeout(Duration::from_secs(3), watchdog_rx.recv()).await;
                                    if let Err(_)| Ok(None) = check_timeout {
                                        // Timeout
                                        // Reply with Ok to trigger the restart
                                        log::debug!("Watchdog kicking the stream");
                                        break AnyResult::Ok(());
                                    }
                                }
                            } => v,
                            result = instance.run_passive_task(|camera| {
                                    let vid_tx = vid.clone();
                                    let aud_tx = aud.clone();
                                    let stream_config = config.clone();
                                    let vid_history = vid_history.clone();
                                    let aud_history = aud_history.clone();
                                    let watchdog_tx = watchdog_tx.clone();
                                    log::debug!("Running Stream Instance Task");
                                    Box::pin(async move {
                                        let mut recieved_iframe = false;
                                        let mut aud_keyframe = false;
                                        let res = async {
                                            let mut prev_ts = Duration::ZERO;
                                            let mut stream_data = camera.start_video(name, 0, strict).await?;
                                            loop {
                                                watchdog_tx.send(()).await?;  // Feed the watchdog
                                                let data = stream_data.get_data().await??;
                                                watchdog_tx.send(()).await?;  // Feed the watchdog

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
                                                    BcMedia::Iframe(BcMediaIframe{data, microseconds, ..}) => {
                                                        prev_ts = Duration::from_micros(microseconds as u64);
                                                        let d = StampedData{
                                                                keyframe: true,
                                                                data: Arc::new(data),
                                                                ts: prev_ts
                                                        };
                                                        let _ = vid_tx.send(d.clone());
                                                        vid_history.send_modify(|history| {
                                                           history.push_back(d);
                                                           while history.len() > 100 {
                                                               history.pop_front();
                                                           }
                                                        });
                                                        recieved_iframe = true;
                                                        aud_keyframe = true;
                                                        log::trace!("Sent Vid Key Frame");
                                                    },
                                                    BcMedia::Pframe(BcMediaPframe{data, microseconds,..}) if recieved_iframe => {
                                                        prev_ts = Duration::from_micros(microseconds as u64);
                                                        let d = StampedData{
                                                            keyframe: false,
                                                            data: Arc::new(data),
                                                            ts: prev_ts
                                                        };
                                                        let _ = vid_tx.send(d.clone());
                                                        vid_history.send_modify(|history| {
                                                           history.push_back(d);
                                                           while history.len() > 100 {
                                                               history.pop_front();
                                                           }
                                                        });
                                                        log::trace!("Sent Vid Frame");
                                                    }
                                                    BcMedia::Aac(BcMediaAac{data, ..}) | BcMedia::Adpcm(BcMediaAdpcm{data,..}) if recieved_iframe => {
                                                        let d = StampedData{
                                                            keyframe: aud_keyframe,
                                                            data: Arc::new(data),
                                                            ts: prev_ts,
                                                        };
                                                        aud_keyframe = false;
                                                        let _ = aud_tx.send(d.clone())?;
                                                        aud_history.send_modify(|history| {
                                                           history.push_back(d);
                                                           while history.len() > 100 {
                                                               history.pop_front();
                                                           }
                                                        });
                                                        log::trace!("Sent Aud Frame");
                                                    },
                                                    _ => {},
                                                }
                                            }
                                        }.await;
                                        Ok(res)
                                    })
                                }) => {
                                match result {
                                    Ok(AnyResult::Ok(())) => {
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
                } => v,
            };
            log::debug!("Stream Thead Stopped: {:?}", r);
            r
        }));

        Ok(me)
    }

    async fn shutdown(&mut self) -> Result<()> {
        log::debug!("StreamData::shutdown Cancel");
        self.cancel.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        Ok(())
    }
}

impl Drop for StreamData {
    fn drop(&mut self) {
        log::trace!("Drop StreamData");
        self.cancel.cancel();
        if let Some(h) = self.handle.take() {
            let _gt = tokio::runtime::Handle::current().enter();
            tokio::task::spawn(async move {
                let _ = h.await;
                log::trace!("Dropped StreamData");
            });
        }
    }
}

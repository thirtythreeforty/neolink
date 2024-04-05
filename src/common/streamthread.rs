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
        oneshot::{channel as oneshot, Sender as OneshotSender},
        watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
    },
    task::JoinHandle,
    time::{sleep, timeout, Duration},
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
    pub(crate) bitrate: u32,
    pub(crate) fps: u32,
}

impl StreamConfig {
    pub(crate) fn vid_ready(&self) -> bool {
        self.resolution[0] > 0
            && self.resolution[1] > 0
            && self.bitrate > 0
            && !matches!(self.vid_format, VidFormat::None)
    }

    pub(crate) fn aud_ready(&self) -> bool {
        self.vid_ready() && !matches!(self.aud_format, AudFormat::None)
    }
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
        const BUFFER_DURATION: Duration = Duration::from_secs(15);
        // At 30fps for 15s with audio is is about 900 frames
        // Therefore we set this buffer to a rather large 2000
        let (vid, _) = broadcast::<StampedData>(2000);
        let (aud, _) = broadcast::<StampedData>(2000);
        let (vid_history, _) = watch::<VecDeque<StampedData>>(VecDeque::new());
        let vid_history = Arc::new(vid_history);
        let (aud_history, _) = watch::<VecDeque<StampedData>>(VecDeque::new());
        let aud_history = Arc::new(aud_history);
        let (resolution, bitrate, fps, fps_table) = instance
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
                        let bitrate_table = encode
                            .bitrate_table
                            .split(',')
                            .filter_map(|c| {
                                let i: Result<u32, _> = c.parse();
                                i.ok()
                            })
                            .collect::<Vec<u32>>();
                        let framerate_table = encode
                            .framerate_table
                            .split(',')
                            .filter_map(|c| {
                                let i: Result<u32, _> = c.parse();
                                i.ok()
                            })
                            .collect::<Vec<u32>>();

                        Ok((
                            [encode.resolution.width, encode.resolution.height],
                            bitrate_table
                                .get(encode.default_bitrate as usize)
                                .copied()
                                .unwrap_or(encode.default_bitrate)
                                * 1024,
                            framerate_table
                                .get(encode.default_framerate as usize)
                                .copied()
                                .unwrap_or(encode.default_framerate),
                            framerate_table.clone(),
                        ))
                    } else {
                        Ok(([0, 0], 0, 0, vec![]))
                    }
                })
            })
            .await?;
        let (config_tx, _) = watch(StreamConfig {
            resolution,
            vid_format: VidFormat::None,
            aud_format: AudFormat::None,
            bitrate,
            fps,
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
        let cam_name = instance.config().await?.borrow().name.clone();
        let print_name = format!("{cam_name}::{name}");
        let strict = me.strict;
        let config = me.config.clone();
        let thread_inuse = me.users.create_deactivated().await?;
        let vid_history = me.vid_history.clone();
        let aud_history = me.aud_history.clone();
        let mut permit = instance.permit().await?;
        me.handle = Some(tokio::task::spawn(async move {
            let r = tokio::select! {
                _ = cancel.cancelled() => {
                    Result::<(), anyhow::Error>::Ok(())
                },
                v = async {
                    loop {
                        let (watchdog_tx, mut watchdog_rx) = mpsc(1);
                        let (watchdog_eat_tx, watchdog_eat_rx) = oneshot();
                        // Give the watchdog his own thread to play in
                        //
                        // This should stop one branch of the select from waking the other
                        // too often
                        let watchdog_print_name = print_name.clone();
                        tokio::task::spawn(async move {
                            let mut check_timeout = timeout(Duration::from_secs(15), watchdog_rx.recv()).await; // Wait longer for the first feed
                            loop {
                                match check_timeout {
                                    Err(_) => {
                                        // Timeout
                                        // Break with Ok to trigger the restart
                                        log::debug!("{watchdog_print_name}: Watchdog kicking the stream");
                                        break;
                                    },
                                    Ok(None) => {
                                        log::debug!("{watchdog_print_name}: Watchdog dropped the stream");
                                        break;
                                    }
                                    Ok(_) => {
                                        // log::debug!("{print_name}: Good Doggo");
                                        check_timeout = timeout(Duration::from_secs(10), watchdog_rx.recv()).await;
                                    }
                                }
                            }
                            // Watch dog is hungry send the kill to the stream thread
                            let _ = watchdog_eat_tx.send(());
                        }) ;

                        tokio::select! {
                            v = thread_inuse.dropped_users() => {
                                // Handles the stop and restart when no active users
                                log::debug!("{print_name}: Streaming STOP");
                                permit.deactivate().await?;
                                v?;
                                thread_inuse.aquired_users().await?; // Wait for new users of the stream
                                permit.activate().await?;
                                log::debug!("{print_name}: Streaming START");
                                AnyResult::Ok(())
                            },
                            _ = watchdog_eat_rx => {
                                sleep(Duration::from_secs(1)).await;
                                AnyResult::Ok(())
                            },
                            result = instance.run_passive_task(|camera| {
                                    let vid_tx = vid.clone();
                                    let aud_tx = aud.clone();
                                    let stream_config = config.clone();
                                    let vid_history = vid_history.clone();
                                    let aud_history = aud_history.clone();
                                    let watchdog_tx = watchdog_tx.clone();
                                    let fps_table = fps_table.clone();
                                    let print_name = print_name.clone();

                                    log::debug!("{print_name}: Running Stream Instance Task");
                                    Box::pin(async move {
                                        // use std::io::Write;
                                        // let mut file = std::fs::File::create("reference.h264")?;
                                        let mut recieved_iframe = false;
                                        let mut aud_keyframe = false;

                                        let res = async {
                                            let mut prev_ts = Duration::ZERO;
                                            let mut stream_data = camera.start_video(name, 0, strict).await?;
                                            loop {
                                                log::debug!("{print_name}:   Waiting for frame");
                                                let data = stream_data.get_data().await??;
                                                log::debug!("{print_name}:   Waiting for Watchdog");
                                                watchdog_tx.send(()).await?;  // Feed the watchdog
                                                log::debug!("{print_name}:   Got frame");

                                                // Update the stream config with any information
                                                match &data {
                                                    BcMedia::InfoV1(info) => {
                                                        stream_config.send_if_modified(|state| {
                                                            let new_fps = fps_table.get(info.fps as usize).copied().unwrap_or(info.fps as u32);
                                                            if state.resolution[0] != info.video_width || state.resolution[1] != info.video_height || new_fps != state.fps  {
                                                                state.resolution[0] = info.video_width;
                                                                state.resolution[1] = info.video_height;
                                                                state.fps = new_fps;
                                                                true
                                                            } else {
                                                                false
                                                            }
                                                        });
                                                    },
                                                    BcMedia::InfoV2(info) => {
                                                        stream_config.send_if_modified(|state| {
                                                            let new_fps = fps_table.get(info.fps as usize).copied().unwrap_or(info.fps as u32);
                                                            if state.resolution[0] != info.video_width || state.resolution[1] != info.video_height || new_fps != state.fps  {
                                                                state.resolution[0] = info.video_width;
                                                                state.resolution[1] = info.video_height;
                                                                state.fps = new_fps;
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
                                                        // let _ = file.write(&frame.data);
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
                                                        // let _ = file.write(&frame.data);
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
                                                        // log::debug!("IFrame: {prev_ts:?}");
                                                        let d = StampedData{
                                                                keyframe: true,
                                                                data: Arc::new(data),
                                                                ts: prev_ts
                                                        };
                                                        let _ = vid_tx.send(d.clone());
                                                        vid_history.send_modify(|history| {
                                                           let drop_time = d.ts.saturating_sub(BUFFER_DURATION);
                                                           history.push_back(d);
                                                           while history.front().is_some_and(|di| di.ts < drop_time) {
                                                               history.pop_front();
                                                           }
                                                        });
                                                        recieved_iframe = true;
                                                        aud_keyframe = true;
                                                        log::trace!("Sent Vid Key Frame");
                                                    },
                                                    BcMedia::Pframe(BcMediaPframe{data, microseconds,..}) if recieved_iframe => {
                                                        prev_ts = Duration::from_micros(microseconds as u64);
                                                        // log::debug!("PFrame: {prev_ts:?}");
                                                        // log::debug!("data: {data:02X?}");
                                                        let d = StampedData{
                                                            keyframe: false,
                                                            data: Arc::new(data),
                                                            ts: prev_ts
                                                        };
                                                        let _ = vid_tx.send(d.clone());
                                                        vid_history.send_modify(|history| {
                                                           let drop_time = d.ts.saturating_sub(BUFFER_DURATION);
                                                           history.push_back(d);
                                                           while history.front().is_some_and(|di| di.ts < drop_time) {
                                                               history.pop_front();
                                                           }
                                                        });
                                                        log::trace!("Sent Vid Frame");
                                                    }
                                                    BcMedia::Aac(BcMediaAac{data, ..}) | BcMedia::Adpcm(BcMediaAdpcm{data,..}) if recieved_iframe => {
                                                        // log::debug!("Audio: {prev_ts:?}");
                                                        let d = StampedData{
                                                            keyframe: aud_keyframe,
                                                            data: Arc::new(data),
                                                            ts: prev_ts,
                                                        };
                                                        aud_keyframe = false;
                                                        let _ = aud_tx.send(d.clone())?;
                                                        aud_history.send_modify(|history| {
                                                           let drop_time = d.ts.saturating_sub(BUFFER_DURATION);
                                                           history.push_back(d);
                                                           while history.front().is_some_and(|di| di.ts < drop_time) {
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
                                        log::debug!("{print_name}: Video Stream Stopped due to no listeners");
                                        break Ok(());
                                    },
                                    Ok(Err(e)) => {
                                        log::debug!("{print_name}: Video Stream Restarting Due to Error: {:?}", e);
                                        AnyResult::Ok(())
                                    },
                                    Err(e) => {
                                        log::debug!("{print_name}: Video Stream Stopped Due to Instance Error: {:?}", e);
                                        break Err(e);
                                    },
                                }
                            },
                        }?;
                    }
                } => v,
            };
            log::debug!("{print_name}: Stream Thead Stopped: {:?}", r);
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

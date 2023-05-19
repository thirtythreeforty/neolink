//! The component that handles passing BcMedia into
//! gstreamer media stream
use anyhow::anyhow;
use futures::{
    future::TryFutureExt,
    stream::{FuturesUnordered, StreamExt},
};
use gstreamer::{prelude::*, ClockTime};
use gstreamer_app::AppSrc;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use is_sorted::IsSorted;
use itertools::Itertools;
use log::*;
use neolink_core::bcmedia::model::*;
use std::{
    collections::{HashMap, VecDeque},
    convert::TryInto,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    time::Duration,
};
use tokio_stream::wrappers::ReceiverStream;

use super::{factory::FactoryCommand, shared::*, AnyResult};
use crate::rtsp::Spring;

type FrameTime = i64;

#[derive(Debug, Clone)]
struct Stamped {
    time: FrameTime,
    data: Vec<Arc<BcMedia>>,
}

#[derive(Debug)]
struct NeoBuffer {
    buf: VecDeque<Stamped>,
    max_size: usize,
}

impl NeoBuffer {
    fn new(max_size: usize) -> Self {
        Self {
            buf: Default::default(),
            max_size,
        }
    }

    fn push(&mut self, media: Arc<BcMedia>) {
        let frame_time = match media.as_ref() {
            BcMedia::Iframe(data) => Some(data.microseconds as FrameTime),
            BcMedia::Pframe(data) => Some(data.microseconds as FrameTime),
            _ => None,
        };
        if let Some(frame_time) = frame_time {
            let mut sorting_vec = vec![];
            let time = frame_time;
            while self
                .buf
                .back()
                .map(|back| back.time >= time)
                .unwrap_or(false)
            {
                sorting_vec.push(self.buf.pop_back().unwrap());
            }
            if sorting_vec
                .last()
                .map(|last| last.time == frame_time)
                .unwrap_or(false)
            {
                sorting_vec.last_mut().unwrap().data.push(media);
            } else {
                sorting_vec.push(Stamped {
                    time,
                    data: vec![media],
                });
            }

            while let Some(sorted_item) = sorting_vec.pop() {
                trace!("Pushing frame with time: {}", sorted_item.time);
                self.buf.push_back(sorted_item);
            }

            debug_assert!(
                IsSorted::is_sorted(&mut self.buf.iter().map(|stamped| stamped.time)),
                "{:?}",
                self.buf
                    .iter()
                    .map(|stamped| stamped.time)
                    .collect::<Vec<_>>()
            );
        } else if let Some(last) = self.buf.back_mut() {
            last.data.push(media);
        }

        while self.buf.len() > self.max_size {
            if self.buf.pop_front().is_none() {
                break;
            }
        }
    }

    fn prep_size(&self) -> usize {
        self.max_size * 2 / 3
    }

    fn live_size(&self) -> usize {
        self.max_size / 3
    }

    pub(crate) fn ready(&self) -> bool {
        self.buf.len() > self.prep_size()
    }

    pub(crate) fn ready_play(&self) -> bool {
        self.buf.len() > self.live_size()
    }

    // fn last_iframe_time(&self) -> Option<FrameTime> {
    //     let (fronts, backs) = self.buf.as_slices();
    //     backs
    //         .iter()
    //         .rev()
    //         .chain(&mut fronts.iter().rev())
    //         .flat_map(|frame| match frame {
    //             BcMedia::Iframe(BcMediaIframe { microseconds, .. }) => Some(*microseconds),
    //             _ => None,
    //         })
    //         .next()
    //         .map(|i| i as FrameTime)
    // }

    // fn first_iframe_time(&self) -> Option<FrameTime> {
    //     let (fronts, backs) = self.buf.as_slices();
    //     fronts
    //         .iter()
    //         .chain(&mut backs.iter())
    //         .flat_map(|frame| match frame {
    //             BcMedia::Iframe(BcMediaIframe { microseconds, .. }) => Some(*microseconds),
    //             _ => None,
    //         })
    //         .next()
    //         .map(|i| i as FrameTime)
    // }

    // fn start_time(&self) -> Option<FrameTime> {
    //     let (fronts, backs) = self.buf.as_slices();
    //     fronts
    //         .iter()
    //         .chain(&mut backs.iter())
    //         .map(|frame| frame.time)
    //         .next()
    // }

    fn end_time(&self) -> Option<FrameTime> {
        let (fronts, backs) = self.buf.as_slices();
        backs
            .iter()
            .rev()
            .chain(&mut fronts.iter().rev())
            .map(|frame| frame.time)
            .next()
    }

    // fn min_time(&self) -> Option<FrameTime> {
    //     let (fronts, backs) = self.buf.as_slices();
    //     fronts
    //         .iter()
    //         .chain(&mut backs.iter())
    //         .flat_map(|frame| match Arc::as_ref(frame) {
    //             BcMedia::Iframe(BcMediaIframe { microseconds, .. }) => Some(*microseconds),
    //             BcMedia::Pframe(BcMediaPframe { microseconds, .. }) => Some(*microseconds),
    //             _ => None,
    //         })
    //         .min()
    //         .map(|i| i as FrameTime)
    // }

    // fn max_time(&self) -> Option<FrameTime> {
    //     let (fronts, backs) = self.buf.as_slices();
    //     backs
    //         .iter()
    //         .rev()
    //         .chain(&mut fronts.iter().rev())
    //         .flat_map(|frame| match Arc::as_ref(frame) {
    //             BcMedia::Iframe(BcMediaIframe { microseconds, .. }) => Some(*microseconds),
    //             BcMedia::Pframe(BcMediaPframe { microseconds, .. }) => Some(*microseconds),
    //             _ => None,
    //         })
    //         .max()
    //         .map(|i| i as FrameTime)
    // }
}

pub(super) struct NeoMediaSenders {
    data_source: ReceiverStream<FactoryCommand>,
    client_source: ReceiverStream<NeoMediaSender>,
    shared: Arc<NeoMediaShared>,
    uid: AtomicU64,
    client_data: HashMap<u64, NeoMediaSender>,
    buffer: NeoBuffer,
}

impl NeoMediaSenders {
    pub(super) fn new(
        shared: Arc<NeoMediaShared>,
        data_source: ReceiverStream<FactoryCommand>,
        client_source: ReceiverStream<NeoMediaSender>,
        buffer_size: usize,
    ) -> Self {
        Self {
            data_source,
            client_source,
            shared,
            uid: Default::default(),
            client_data: Default::default(),
            buffer: NeoBuffer::new(buffer_size),
        }
    }

    async fn handle_new_client(&mut self, mut client: NeoMediaSender) -> AnyResult<()> {
        if client.vid.is_some() || client.aud.is_some() {
            // Must have at least one type of source
            //
            // If not this is the dummy stream
            // we don't keep a reference to that
            client.initialise(&self.buffer).await?;
            self.client_data
                .insert(self.uid.fetch_add(1, Ordering::Relaxed), client);
        }
        Ok(())
    }

    async fn update_mediatypes(&self, data: &BcMedia) {
        match data {
            BcMedia::Iframe(BcMediaIframe {
                video_type: VideoType::H264,
                ..
            }) => {
                if !matches!(*self.shared.vid_format.read().await, VidFormats::H264) {
                    *self.shared.vid_format.write().await = VidFormats::H264;
                }
            }
            BcMedia::Pframe(BcMediaPframe {
                video_type: VideoType::H264,
                ..
            }) => {
                if !matches!(*self.shared.vid_format.read().await, VidFormats::H264) {
                    *self.shared.vid_format.write().await = VidFormats::H264;
                }
            }
            BcMedia::Iframe(BcMediaIframe {
                video_type: VideoType::H265,
                ..
            }) => {
                if !matches!(*self.shared.vid_format.read().await, VidFormats::H265) {
                    *self.shared.vid_format.write().await = VidFormats::H265;
                }
            }
            BcMedia::Pframe(BcMediaPframe {
                video_type: VideoType::H265,
                ..
            }) => {
                if !matches!(*self.shared.vid_format.read().await, VidFormats::H265) {
                    *self.shared.vid_format.write().await = VidFormats::H265;
                }
            }
            BcMedia::Aac(_) => {
                if !matches!(*self.shared.aud_format.read().await, AudFormats::Aac) {
                    *self.shared.aud_format.write().await = AudFormats::Aac;
                }
            }
            BcMedia::Adpcm(data) => {
                if !matches!(*self.shared.aud_format.read().await, AudFormats::Adpcm(_)) {
                    *self.shared.aud_format.write().await =
                        AudFormats::Adpcm(data.data.len().try_into().unwrap());
                }
            }
            _ => {}
        }
    }

    async fn handle_new_data(&mut self, data: BcMedia) -> AnyResult<()> {
        // trace!("Handle new data");
        self.update_mediatypes(&data).await;
        let time = match &data {
            BcMedia::Iframe(BcMediaIframe { microseconds, .. }) => *microseconds as FrameTime,
            BcMedia::Pframe(BcMediaPframe { microseconds, .. }) => *microseconds as FrameTime,
            _ => self.buffer.end_time().unwrap_or(0),
        };

        let data = Arc::new(data);

        let end_time = self.buffer.end_time();
        let frame_time = time;
        // Ocassionally the camera will make a jump in timestamps of about 15s (on sub 9s on main)
        // This could mean that it runs on some fixed sized buffer
        if let Some(end_time) = end_time {
            let delta_frame = (self.buffer.buf.back().unwrap().time
                - self.buffer.buf.front().unwrap().time)
                / self.buffer.buf.len() as i64;
            let delta_time = frame_time - end_time - delta_frame;
            let delta_duration = Duration::from_micros(delta_time.unsigned_abs());
            if delta_duration > Duration::from_secs(1) {
                trace!(
                    "Reschedule buffer due to jump: {:?}, Prev: {}, New: {}",
                    delta_duration,
                    end_time,
                    frame_time
                );
                trace!("Adjusting master: {}", self.buffer.buf.len());
                for frame in self.buffer.buf.iter_mut() {
                    let old_frame_time = frame.time;
                    frame.time = frame.time.saturating_add(delta_time);
                    trace!(
                        "  - New frame time: {} -> {} (target {})",
                        old_frame_time,
                        frame.time,
                        frame_time
                    );
                }

                for (_, client) in self.client_data.iter_mut() {
                    for frame in client.buffer.buf.iter_mut() {
                        frame.time = frame.time.saturating_add(delta_time);
                    }
                    client.start_time.mod_value(delta_time as f64);
                }
            }
        }

        for client_data in self.client_data.values_mut() {
            client_data.add_data(data.clone()).await?;
        }
        self.buffer.push(data);

        self.shared
            .buffer_ready
            .store(self.buffer.ready(), Ordering::Relaxed);

        Ok(())
    }

    async fn init_clients(&mut self) -> AnyResult<()> {
        let (client_data, buffer) = (&mut self.client_data, &self.buffer);
        let keys_to_remove = client_data
            .iter_mut()
            .map(|(&key, client)| {
                client
                    .initialise(buffer)
                    .map_ok(|_| None)
                    .unwrap_or_else(move |e| {
                        trace!("Could not init client: {:?}", e);
                        Some(key)
                    })
            })
            .collect::<FuturesUnordered<_>>()
            .filter_map(|a| async move { a })
            .collect::<Vec<_>>()
            .await;
        for key in keys_to_remove.iter() {
            client_data.remove(key);
        }
        Ok(())
    }

    async fn process_client_commands(&mut self) -> AnyResult<()> {
        let (client_data, buffer) = (&mut self.client_data, &self.buffer);
        let keys_to_remove = client_data
            .iter_mut()
            .map(|(&key, client)| {
                client
                    .process_commands(buffer)
                    .map_ok(|_| None)
                    .unwrap_or_else(move |e| {
                        trace!("Could not process client command: {:?}", e);
                        Some(key)
                    })
            })
            .collect::<FuturesUnordered<_>>()
            .filter_map(|a| async move { a })
            .collect::<Vec<_>>()
            .await;
        for key in keys_to_remove.iter() {
            client_data.remove(key);
        }
        Ok(())
    }

    async fn process_client_update(&mut self) -> AnyResult<()> {
        let client_data = &mut self.client_data;
        let keys_to_remove = client_data
            .iter_mut()
            .map(|(&key, client)| {
                client.update().map_ok(|_| None).unwrap_or_else(move |e| {
                    trace!("Could not update client: {:?}", e);
                    Some(key)
                })
            })
            .collect::<FuturesUnordered<_>>()
            .filter_map(|a| async move { a })
            .collect::<Vec<_>>()
            .await;
        for key in keys_to_remove.iter() {
            client_data.remove(key);
        }
        Ok(())
    }

    async fn clear_buffer(&mut self) -> AnyResult<()> {
        self.buffer.buf.clear();
        self.shared.buffer_ready.store(false, Ordering::Relaxed);
        for client in self.client_data.values_mut() {
            // Set them into the non init state
            // This will make them wait for the
            // buffer to be enough then jump to live
            client.buffer.buf.clear();
            client.inited = false;
        }
        Ok(())
    }

    async fn update(&mut self) -> AnyResult<()> {
        self.buffer.max_size = self.shared.get_buffer_size();
        self.init_clients().await?;
        self.process_client_commands().await?;
        self.process_client_update().await?;

        Ok(())
    }

    pub(super) async fn run(&mut self) -> AnyResult<()> {
        let mut interval = tokio::time::interval(Duration::from_millis(40)); // 25 FPS
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::task::yield_now().await;
            self.shared
                .number_of_clients
                .store(self.client_data.len(), Ordering::Relaxed);
            tokio::select! {
                _ = interval.tick() => {
                    self.update().await
                },
                Some(v) = self.data_source.next() => {
                    match v {
                        FactoryCommand::BcMedia(media) => {
                            let frame_time = match &media {
                                BcMedia::Iframe(data) => Some(Duration::from_micros(data.microseconds as u64)),
                                BcMedia::Pframe(data) => Some(Duration::from_micros(data.microseconds as u64)),
                                _ => None,
                            };
                            if let Some(frame_time) = frame_time {
                                trace!("Got frame at {:?}", frame_time);
                            }

                            self.handle_new_data(media).await?;
                        },
                        FactoryCommand::ClearBuffer => {
                            self.clear_buffer().await?;
                        },
                        FactoryCommand::JumpToLive => {
                            for client in self.client_data.values_mut() {
                                // Set them into the non init state
                                // This will make them wait for the
                                // buffer to be enough then jump to live
                                let _ = client.jump_to_live().await;
                            }
                        },
                        FactoryCommand::Pause => {
                            for client in self.client_data.values_mut() {
                                client.playing = false;
                            }
                        },
                        FactoryCommand::Resume => {
                            for client in self.client_data.values_mut() {
                                client.playing = true;
                                let _ = client.jump_to_live().await;
                            }
                        },
                    }
                    Ok(())
                },
                Some(v) = self.client_source.next() => {
                    self.handle_new_client(v).await
                },
                else => {
                    Err(anyhow!("Sender data source closed"))
                }
            }?;
        }
    }
}

pub(super) enum NeoMediaSenderCommand {
    Seek(Option<i64>, u64),
    Pause,
    Resume,
}
#[derive(Debug)]
pub(super) struct NeoMediaSender {
    start_time: Spring,
    live_offset: FrameTime,
    buffer: NeoBuffer,
    vid: Option<AppSrc>,
    aud: Option<AppSrc>,
    command_reciever: Receiver<NeoMediaSenderCommand>,
    command_sender: Sender<NeoMediaSenderCommand>,
    inited: bool,
    playing: bool,
    refilling: bool,
    use_smoothing: bool,
}

impl NeoMediaSender {
    pub(super) fn new(buffer_size: usize, use_smoothing: bool) -> Self {
        let (tx, rx) = channel(30);
        Self {
            start_time: Spring::new(0.0, 0.0, 10.0),
            live_offset: 0,
            buffer: NeoBuffer::new(buffer_size),
            vid: None,
            aud: None,
            command_reciever: rx,
            command_sender: tx,
            inited: false,
            playing: true,
            refilling: false,
            use_smoothing,
        }
    }

    async fn add_data(&mut self, data: Arc<BcMedia>) -> AnyResult<()> {
        self.buffer.push(data);
        Ok(())
    }

    pub(super) fn update_vid(&mut self, source: AppSrc) {
        self.vid.replace(source);
    }

    pub(super) fn update_aud(&mut self, source: AppSrc) {
        self.aud.replace(source);
    }

    pub(super) fn get_commader(&self) -> Sender<NeoMediaSenderCommand> {
        self.command_sender.clone()
    }

    fn target_live_for(buffer: &NeoBuffer) -> Option<FrameTime> {
        let target_idx = buffer.live_size();
        let stamps = buffer.buf.iter().map(|item| item.time).collect::<Vec<_>>();

        if stamps.len() >= target_idx {
            let target_frame = stamps.len().saturating_sub(target_idx);
            stamps.get(target_frame).copied()
        } else if stamps.len() > 5 {
            // Approximate it's location
            let fraction = target_idx as f64 / stamps.len() as f64;
            if let (Some(st), Some(et)) = (stamps.first(), stamps.last()) {
                Some(et - ((et - st) as f64 * fraction) as FrameTime)
            } else {
                None
            }
        } else {
            trace!("Not enough timestamps for target live: {:?}", stamps);
            if let Some(st) = stamps.first() {
                trace!("Setting to 1s behind first frame in buffer");
                Some(st - Duration::from_secs(1).as_micros() as FrameTime)
            } else {
                None
            }
        }
    }

    fn target_live(&self) -> Option<FrameTime> {
        Self::target_live_for(&self.buffer)
    }

    async fn jump_to_live(&mut self) -> AnyResult<()> {
        let target_time = self.target_live();

        if let Some(target_time) = target_time {
            if let Some(et) = self.buffer.end_time() {
                trace!(
                    "Buffer stamps: {:?}",
                    self.buffer
                        .buf
                        .iter()
                        .fold(Vec::<FrameTime>::new(), |mut acc, item| {
                            if let Some(last) = acc.last() {
                                if *last < item.time {
                                    acc.push(item.time);
                                }
                            } else {
                                acc.push(item.time);
                            }
                            acc
                        })
                );
                debug!(
                    "Minimum Latency: {:?} ({:?} - {:?})",
                    Duration::from_micros(et.saturating_sub(target_time).max(0) as u64),
                    Duration::from_micros(et.max(0) as u64),
                    Duration::from_micros(target_time.max(0) as u64),
                );
            }
            let runtime = self.get_runtime().unwrap_or(0);

            self.start_time.reset_to((target_time - runtime) as f64);
            trace!(
                "Jumped to live: New start time: {:?}",
                Duration::from_micros(self.start_time.value_u64()),
            );
        }

        Ok(())
    }

    async fn update_starttime(&mut self) -> AnyResult<()> {
        self.start_time.update().await;

        if self.use_smoothing {
            if let (Some(runtime), Some(target_time)) = (self.get_runtime(), self.target_live()) {
                self.start_time.set_target((target_time - runtime) as f64);
            }
        }
        Ok(())
    }

    async fn seek(
        &mut self,
        _original_runtime: Option<FrameTime>,
        _target_runtime: FrameTime,
        _master_buffer: &NeoBuffer,
    ) -> AnyResult<()> {
        self.jump_to_live().await?;
        Ok(())
    }

    async fn process_commands(&mut self, master_buffer: &NeoBuffer) -> AnyResult<()> {
        if self.inited {
            if let Ok(command) = self.command_reciever.try_recv() {
                match command {
                    NeoMediaSenderCommand::Seek(runtime, dest) => {
                        debug!("Got Seek Request: {:?}", Duration::from_micros(dest));
                        self.seek(runtime, dest as FrameTime, master_buffer).await?;
                    }
                    NeoMediaSenderCommand::Pause => {
                        if self.playing {
                            debug!("Pausing");
                            self.playing = false;
                        }
                    }
                    NeoMediaSenderCommand::Resume => {
                        if !self.playing {
                            debug!("Resuming");
                            self.playing = true;
                            self.jump_to_live().await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn initialise(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
        if !self.inited && buffer.ready() {
            if let Some(target_time) = Self::target_live_for(buffer) {
                // Minimum buffer
                self.inited = true;
                self.buffer.buf.clear();

                let mut buffer_iter = buffer.buf.iter().cloned();
                let preprocess = buffer_iter
                    .take_while_ref(|item| item.time < target_time)
                    .collect::<Vec<_>>();
                let mut buffer = buffer_iter.collect::<Vec<_>>();
                self.start_time.reset_to(target_time as f64);

                // Send preprocess now
                self.send_buffers(preprocess.as_slice()).await?;
                trace!("Preprocessed");
                // Send these later
                for frame in buffer.drain(..) {
                    self.buffer.buf.push_back(frame);
                }
                trace!("Buffer filled");

                self.jump_to_live().await?;
            } else {
                debug!("Buffer not ready to init: {}", buffer.buf.len());
            }
        } else if !self.inited {
            debug!("Buffer not ready to init: {}", buffer.buf.len());
        }
        Ok(())
    }

    fn get_runtime(&self) -> Option<FrameTime> {
        if let Some(appsrc) = self.vid.as_ref() {
            if let Some(clock) = appsrc.clock() {
                if let Some(time) = clock.time() {
                    if let Some(base_time) = appsrc.base_time() {
                        let runtime = time.saturating_sub(base_time);
                        let res = Some(
                            (runtime.useconds() as FrameTime)
                                .saturating_add(self.live_offset)
                                .max(0),
                        );
                        // debug!("base_time: {:?}", base_time);
                        // debug!("time: {:?}", time);
                        // debug!("runtime: {:?}", runtime);
                        // debug!("Final runtime: {:?}", res);
                        trace!(
                            "Runtime: {:?}, Offset: {:?}, Offseted Runtime: {:?}",
                            runtime,
                            self.live_offset,
                            res
                        );
                        return res;
                    }
                }
            }
        }
        None
    }

    fn get_buftime(&self) -> Option<FrameTime> {
        self.get_runtime().map(|time| self.runtime_to_buftime(time))
    }

    fn runtime_to_buftime(&self, runtime: FrameTime) -> FrameTime {
        runtime.saturating_add(self.start_time.value_i64())
    }

    fn buftime_to_runtime(&self, buftime: FrameTime) -> FrameTime {
        buftime.saturating_sub(self.start_time.value_i64()).max(0)
    }

    async fn update(&mut self) -> AnyResult<()> {
        if self.buffer.buf.len() >= self.buffer.max_size * 9 / 10 {
            debug!("Buffer overfull");
            self.jump_to_live().await?;
        }
        if self.refilling && self.buffer.ready_play() {
            self.refilling = false;
            self.jump_to_live().await?;
        } else if self.refilling {
            trace!(
                "Refilling: {}/{} ({:.2}%)",
                self.buffer.buf.len(),
                self.buffer.live_size(),
                self.buffer.buf.len() as f32 / (self.buffer.live_size()) as f32 * 100.0
            );
        } else if !self.refilling && self.inited && self.playing {
            self.update_starttime().await?;
            // Check app src is live
            if !self
                .vid
                .as_ref()
                .map(|x| x.pads().iter().all(|pad| pad.is_linked()))
                .unwrap_or(false)
            {
                return Err(anyhow!("Vid src is closed"));
            }

            // Check if buffers are ok
            if self.buffer.buf.len() <= self.buffer.max_size / 10 {
                warn!(
                    "Buffer exhausted. Not enough data from Camera. Pausing RTSP until refilled."
                );
                info!("Try reducing your Max Bitrate using the offical app");
                self.refilling = true;
            } else {
                trace!("Buffer size: {}", self.buffer.buf.len());
            }

            // Send buffers
            let mut buffers = vec![];
            if let Some(buftime) = self.get_buftime() {
                // debug!("Update: buftime: {}", buf time);
                while self
                    .buffer
                    .buf
                    .front()
                    .map(|data| data.time <= buftime)
                    .unwrap_or(false)
                {
                    tokio::task::yield_now().await;
                    match self.buffer.buf.pop_front() {
                        Some(frame) => {
                            buffers.push(frame);
                        }
                        None => break,
                    }
                }
            }

            tokio::task::yield_now().await;
            // collect certain frames
            self.send_buffers(&buffers).await?;
        }

        Ok(())
    }

    async fn send_buffers(&mut self, medias: &[Stamped]) -> AnyResult<()> {
        if medias.is_empty() {
            return Ok(());
        }
        tokio::task::yield_now().await;
        let mut vid_buffers: Vec<(FrameTime, Vec<u8>)> = vec![];
        let mut aud_buffers: Vec<(FrameTime, Vec<u8>)> = vec![];
        for media_sets in medias.iter() {
            tokio::task::yield_now().await;
            for media in media_sets.data.iter() {
                let buffer = match media.as_ref() {
                    BcMedia::Iframe(_) | BcMedia::Pframe(_) => Some(&mut vid_buffers),
                    BcMedia::Aac(_) | BcMedia::Adpcm(_) => Some(&mut aud_buffers),
                    _ => None,
                };
                let data = match media.as_ref() {
                    BcMedia::Iframe(data) => Some(&data.data),
                    BcMedia::Pframe(data) => Some(&data.data),
                    BcMedia::Aac(data) => Some(&data.data),
                    BcMedia::Adpcm(data) => Some(&data.data),
                    _ => None,
                };
                if let (Some(data), Some(buffer)) = (data, buffer) {
                    let next_time = media_sets.time;
                    if let Some(last) = buffer.last_mut() {
                        let last_time = last.0;
                        if next_time == last_time {
                            last.1.extend(data.iter().copied());
                        } else {
                            buffer.push((next_time, data.clone()))
                        }
                    } else {
                        buffer.push((next_time, data.clone()))
                    }
                }
            }
        }
        tokio::task::yield_now().await;
        tokio::try_join!(
            async {
                if !vid_buffers.is_empty() {
                    // debug!("Sending video buffers: {}", vid_buffers.len());
                    if let Some(appsrc) = self.vid.clone() {
                        let buffers = {
                            let mut buffers = gstreamer::BufferList::new_sized(vid_buffers.len());
                            {
                                let buffers_ref = buffers.get_mut().unwrap();
                                for (time, buf) in vid_buffers.drain(..) {
                                    tokio::task::yield_now().await;
                                    let runtime = self.buftime_to_runtime(time);
                                    trace!(
                                        "  - Sending vid frame at time {} ({:?} Expect: {:?})",
                                        time,
                                        Duration::from_micros(runtime as u64),
                                        self.get_runtime().map(|i| Duration::from_micros(i as u64))
                                    );

                                    let gst_buf = {
                                        let mut gst_buf =
                                            gstreamer::Buffer::with_size(buf.len()).unwrap();
                                        {
                                            let gst_buf_mut = gst_buf.get_mut().unwrap();

                                            let time = ClockTime::from_useconds(
                                                runtime.try_into().unwrap(),
                                            );
                                            gst_buf_mut.set_dts(time);
                                            let mut gst_buf_data =
                                                gst_buf_mut.map_writable().unwrap();
                                            gst_buf_data.copy_from_slice(buf.as_slice());
                                        }
                                        gst_buf
                                    };
                                    buffers_ref.add(gst_buf);
                                }
                            }
                            buffers
                        };

                        let res = tokio::task::spawn_blocking(move || {
                            // debug!("  - Pushing buffer: {}", buffers.len());
                            appsrc
                                .push_buffer_list(buffers.copy())
                                .map(|_| ())
                                .map_err(|_| anyhow!("Could not push buffer to appsrc"))
                        })
                        .await;
                        match &res {
                            Err(e) => {
                                debug!("Paniced on send buffer list: {:?}", e);
                            }
                            Ok(Err(e)) => {
                                debug!("Failed to send buffer list: {:?}", e);
                            }
                            Ok(Ok(_)) => {}
                        };
                        res??;
                    }
                }
                AnyResult::Ok(())
            },
            async {
                if !aud_buffers.is_empty() {
                    if let Some(appsrc) = self.aud.clone() {
                        let buffers = {
                            let mut buffers = gstreamer::BufferList::new_sized(aud_buffers.len());
                            {
                                let buffers_ref = buffers.get_mut().unwrap();
                                for (time, buf) in aud_buffers.drain(..) {
                                    tokio::task::yield_now().await;
                                    let runtime = self.buftime_to_runtime(time);

                                    let gst_buf = {
                                        let mut gst_buf =
                                            gstreamer::Buffer::with_size(buf.len()).unwrap();
                                        {
                                            let gst_buf_mut = gst_buf.get_mut().unwrap();

                                            let time = ClockTime::from_useconds(
                                                runtime.try_into().unwrap(),
                                            );
                                            gst_buf_mut.set_dts(time);
                                            let mut gst_buf_data =
                                                gst_buf_mut.map_writable().unwrap();
                                            gst_buf_data.copy_from_slice(buf.as_slice());
                                        }
                                        gst_buf
                                    };
                                    buffers_ref.add(gst_buf);
                                }
                            }
                            buffers
                        };

                        let res = tokio::task::spawn_blocking(move || {
                            appsrc
                                .push_buffer_list(buffers.copy())
                                .map(|_| ())
                                .map_err(|_| anyhow!("Could not push buffer to appsrc"))
                        })
                        .await;
                        match &res {
                            Err(e) => {
                                debug!("Paniced on send buffer list: {:?}", e);
                            }
                            Ok(Err(e)) => {
                                debug!("Failed to send buffer list: {:?}", e);
                            }
                            Ok(Ok(_)) => {}
                        };
                        res??;
                    }
                }
                AnyResult::Ok(())
            }
        )?;
        Ok(())
    }
}

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

const BUFFER_SIZE: usize = 100;

#[derive(Debug, Clone)]
struct Stamped {
    time: FrameTime,
    data: Arc<BcMedia>,
}

#[derive(Default, Debug)]
struct NeoBuffer {
    buf: VecDeque<Stamped>,
}

impl NeoBuffer {
    fn push(&mut self, item: Stamped) {
        // Sort time
        // debug!("sorting");
        let mut sorting_vec = vec![];
        let time = item.time;
        while self
            .buf
            .back()
            .map(|back| back.time > time)
            .unwrap_or(false)
        {
            sorting_vec.push(self.buf.pop_back().unwrap());
        }
        sorting_vec.push(item);

        for sorted_item in sorting_vec.drain(..) {
            // debug!("Pushing frame with time: {}", sorted_item.time);
            self.buf.push_back(sorted_item);
        }
        while self.buf.len() > BUFFER_SIZE {
            if self.buf.pop_front().is_none() {
                break;
            }
        }
    }

    pub(crate) fn ready(&self) -> bool {
        self.buf.len() > BUFFER_SIZE * 2 / 3
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

    fn start_time(&self) -> Option<FrameTime> {
        let (fronts, backs) = self.buf.as_slices();
        fronts
            .iter()
            .chain(&mut backs.iter())
            .map(|frame| frame.time)
            .next()
    }

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
    ) -> Self {
        Self {
            data_source,
            client_source,
            shared,
            uid: Default::default(),
            client_data: Default::default(),
            buffer: Default::default(),
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
        // debug!("Handle new data");
        self.update_mediatypes(&data).await;
        let time = match &data {
            BcMedia::Iframe(BcMediaIframe { microseconds, .. }) => *microseconds as FrameTime,
            BcMedia::Pframe(BcMediaPframe { microseconds, .. }) => *microseconds as FrameTime,
            _ => self.buffer.end_time().unwrap_or(0),
        };

        let data = Stamped {
            time,
            data: Arc::new(data),
        };
        for client_data in self.client_data.values_mut() {
            client_data.add_data(data.clone()).await?;
        }

        let end_time = self.buffer.end_time();
        let frame_time = data.time;
        // Ocassionally the camera will make a jump in timestamps of about 15s (on sub 9s on main)
        // This could mean that it runs on some fixed sized buffer
        if let Some(end_time) = end_time {
            let delta_time = frame_time - end_time - -1;
            let delta_duration = Duration::from_micros(delta_time.unsigned_abs());
            if delta_duration > Duration::from_secs(1) {
                debug!(
                    "Reschedule buffer due to jump: {:?}, Prev: {}, New: {}",
                    delta_duration, end_time, frame_time
                );
                for frame in self.buffer.buf.iter_mut() {
                    frame.time = frame.time.saturating_add(delta_time);
                }

                for (_, client) in self.client_data.iter_mut() {
                    for frame in client.buffer.buf.iter_mut() {
                        frame.time = frame.time.saturating_add(delta_time);
                    }
                    client.jump_to_live().await?;
                }
            }
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
                        debug!("Could not init client: {:?}", e);
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
                        debug!("Could not process client command: {:?}", e);
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
                    debug!("Could not update client: {:?}", e);
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
                                client.inited = false;
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
}

impl NeoMediaSender {
    pub(super) fn new() -> Self {
        let (tx, rx) = channel(30);
        Self {
            start_time: Spring::new(0.0, 0.0, 10.0),
            live_offset: 0,
            buffer: NeoBuffer::default(),
            vid: None,
            aud: None,
            command_reciever: rx,
            command_sender: tx,
            inited: false,
            playing: true,
        }
    }

    async fn add_data(&mut self, data: Stamped) -> AnyResult<()> {
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

    fn target_live(&self) -> Option<FrameTime> {
        let target_idx = BUFFER_SIZE / 3;
        if self.buffer.buf.len() >= target_idx {
            let target_frame = self.buffer.buf.len().saturating_sub(target_idx);
            self.buffer.buf.get(target_frame).map(|frame| frame.time)
        } else {
            // Approximate it's location
            let fraction = target_idx as f64 / self.buffer.buf.len() as f64;
            if let (Some(st), Some(et)) = (self.buffer.start_time(), self.buffer.end_time()) {
                Some(et - ((et - st) as f64 * fraction) as FrameTime)
            } else {
                None
            }
        }
    }

    async fn jump_to_live(&mut self) -> AnyResult<()> {
        if self.inited {
            let target_time = self.target_live();

            if let Some(target_time) = target_time {
                if let Some(et) = self.buffer.end_time() {
                    debug!(
                        "Minimum Latency: {:?}",
                        Duration::from_micros(et.saturating_sub(target_time).max(0) as u64)
                    );
                }
                let runtime = self.get_runtime().unwrap_or(0);

                self.start_time.reset_to((target_time - runtime) as f64);
                debug!(
                    "Jumped to live: New start time: {:?}",
                    Duration::from_micros(self.start_time.value_u64()),
                );
            }
        }

        Ok(())
    }

    async fn update_starttime(&mut self) -> AnyResult<()> {
        self.start_time.update().await;

        if let (Some(runtime), Some(target_time)) = (self.get_runtime(), self.target_live()) {
            self.start_time.set_target((target_time - runtime) as f64);
        }
        Ok(())
    }

    async fn seek(
        &mut self,
        _original_runtime: Option<FrameTime>,
        target_runtime: FrameTime,
        master_buffer: &NeoBuffer,
    ) -> AnyResult<()> {
        if let Some(current_runtime) = self.get_runtime() {
            self.live_offset = self
                .live_offset
                .saturating_add(target_runtime.saturating_sub(current_runtime));
            trace!("Old runtime: {}", current_runtime);
            trace!("Target runtime: {}", target_runtime);
            trace!("Offset: {}", self.live_offset);
            trace!("New runtime: {:?}", self.get_runtime());
            if let Some(new_buftime) = self.get_buftime() {
                self.buffer.buf.clear();
                for frame in master_buffer.buf.iter().filter(|f| f.time >= new_buftime) {
                    self.buffer.buf.push_back(frame.clone());
                }
            }
        }
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
            // Minimum buffer
            self.inited = true;

            // Split buffer into 2/3 of preprocess
            // and one third for the future buffer
            let split_idx = buffer.buf.len() * 2 / 3;
            let rebuf = buffer.buf.iter().cloned().collect::<Vec<_>>();
            let (preprocess, buffer) = rebuf.split_at(split_idx);
            let start_ms = buffer
                .first()
                .map(|data| data.time as f64)
                .expect("Buffer should have a start time");
            self.start_time.reset_to(start_ms);

            // Send preprocess now
            self.send_buffers(preprocess).await?;
            debug!("Preprocessed");
            // Send these later
            for frame in buffer.iter() {
                self.buffer.push(frame.clone());
            }
            debug!("Buffer filled");

            self.jump_to_live().await?;
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
        self.update_starttime().await?;
        let mut buffers = vec![];
        if !self
            .vid
            .as_ref()
            .map(|x| x.pads().iter().all(|pad| pad.is_linked()))
            .unwrap_or(false)
        {
            return Err(anyhow!("Vid src is closed"));
        }
        const LATENCY: FrameTime = Duration::from_millis(250).as_micros() as FrameTime;
        if let Some(buftime) = self.get_buftime().map(|i| i.saturating_add(LATENCY)) {
            // debug!("Update: buftime: {}", buftime);
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

        Ok(())
    }

    async fn send_buffers(&mut self, medias: &[Stamped]) -> AnyResult<()> {
        if medias.is_empty() {
            return Ok(());
        }
        if self.inited && self.playing {
            tokio::task::yield_now().await;
            let mut vid_buffers: Vec<(FrameTime, Vec<u8>)> = vec![];
            let mut aud_buffers: Vec<(FrameTime, Vec<u8>)> = vec![];
            for media in medias.iter() {
                tokio::task::yield_now().await;
                let buffer = match media.data.as_ref() {
                    BcMedia::Iframe(_) | BcMedia::Pframe(_) => Some(&mut vid_buffers),
                    BcMedia::Aac(_) | BcMedia::Adpcm(_) => Some(&mut aud_buffers),
                    _ => None,
                };
                let data = match media.data.as_ref() {
                    BcMedia::Iframe(data) => Some(&data.data),
                    BcMedia::Pframe(data) => Some(&data.data),
                    BcMedia::Aac(data) => Some(&data.data),
                    BcMedia::Adpcm(data) => Some(&data.data),
                    _ => None,
                };
                if let (Some(data), Some(buffer)) = (data, buffer) {
                    let next_time = media.time;
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
            tokio::task::yield_now().await;
            tokio::try_join!(
                async {
                    if !vid_buffers.is_empty() {
                        // debug!("Sending video buffers: {}", vid_buffers.len());
                        if let Some(appsrc) = self.vid.clone() {
                            let buffers = {
                                let mut buffers =
                                    gstreamer::BufferList::new_sized(vid_buffers.len());
                                {
                                    let buffers_ref = buffers.get_mut().unwrap();
                                    for (time, buf) in vid_buffers.drain(..) {
                                        tokio::task::yield_now().await;
                                        let runtime = self.buftime_to_runtime(time);
                                        let _actual_runtime = self
                                            .get_runtime()
                                            .map(|i| Duration::from_micros(i as u64));
                                        // debug!(
                                        //     "  - Sending vid frame at time {} ({:?} Expect: {:?})",
                                        //     time,
                                        //     Duration::from_micros(runtime as u64),
                                        //     actual_runtime
                                        // );

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
                                let mut buffers =
                                    gstreamer::BufferList::new_sized(aud_buffers.len());
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
        }
        Ok(())
    }
}

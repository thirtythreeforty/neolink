//! The component that handles passing BcMedia into
//! gstreamer media stream
use anyhow::anyhow;
use futures::stream::StreamExt;
use gstreamer::{prelude::*, ClockTime};
use gstreamer_app::AppSrc;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use log::*;
use neolink_core::bcmedia::model::*;
use std::{
    collections::{
        VecDeque,
        {hash_map::Entry, HashMap},
    },
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

#[derive(Debug)]
struct Stamped {
    time: FrameTime,
    data: BcMedia,
}

#[derive(Default, Debug)]
struct NeoBuffer {
    buf: VecDeque<Arc<Stamped>>,
}

impl NeoBuffer {
    fn push(&mut self, item: Arc<Stamped>) {
        self.buf.push_back(item);
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

        let data = Arc::new(Stamped { time, data });
        for client_data in self.client_data.values_mut() {
            client_data.add_data(data.clone()).await?;
        }

        let end_time = self.buffer.end_time();
        let frame_time = data.time;
        if let Some(end_time) = end_time {
            let delta_time = end_time - frame_time;
            let delta_duration = Duration::from_micros(delta_time.unsigned_abs());
            if delta_duration > Duration::from_secs(1) {
                debug!("Clearing buffer due to jump");
                self.clear_buffer().await?;
            }
        }

        self.buffer.push(data);

        self.shared
            .buffer_ready
            .store(self.buffer.ready(), Ordering::Relaxed);

        Ok(())
    }

    async fn init_clients(&mut self) -> AnyResult<()> {
        for key in self.client_data.keys().copied().collect::<Vec<_>>() {
            tokio::task::yield_now().await;
            match self.client_data.entry(key) {
                Entry::Occupied(mut occ) => {
                    if let Err(e) = occ.get_mut().initialise(&self.buffer).await {
                        debug!("Could not init client: {:?}", e);
                        occ.remove();
                        continue;
                    }
                }
                Entry::Vacant(_) => {}
            }
        }
        Ok(())
    }

    async fn process_client_commands(&mut self) -> AnyResult<()> {
        for key in self.client_data.keys().copied().collect::<Vec<_>>() {
            tokio::task::yield_now().await;
            match self.client_data.entry(key) {
                Entry::Occupied(mut occ) => {
                    if let Err(e) = occ.get_mut().process_commands().await {
                        debug!("Could not process client command: {:?}", e);
                        occ.remove();
                        continue;
                    }
                }
                Entry::Vacant(_) => {}
            }
        }
        Ok(())
    }

    async fn process_client_update(&mut self) -> AnyResult<()> {
        for key in self.client_data.keys().copied().collect::<Vec<_>>() {
            tokio::task::yield_now().await;
            match self.client_data.entry(key) {
                Entry::Occupied(mut occ) => {
                    if let Err(e) = occ.get_mut().update().await {
                        debug!("Could not update client: {:?}", e);
                        occ.remove();
                        continue;
                    }
                }
                Entry::Vacant(_) => {}
            }
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
    Seek(u64),
    Pause,
    Resume,
}
#[derive(Debug)]
pub(super) struct NeoMediaSender {
    start_time: Spring,
    buffer: NeoBuffer,
    vid: Option<AppSrc>,
    aud: Option<AppSrc>,
    command_reciever: Receiver<NeoMediaSenderCommand>,
    command_sender: Sender<NeoMediaSenderCommand>,
    inited: bool,
    playing: bool,
}

#[allow(dead_code)]
enum JumpMethod {
    MiddleIFrame,
    BufferPerunit(f32),
}

impl NeoMediaSender {
    pub(super) fn new() -> Self {
        let (tx, rx) = channel(30);
        Self {
            start_time: Spring::new(0.0, 0.0, 2.5),
            buffer: NeoBuffer::default(),

            vid: None,
            aud: None,
            command_reciever: rx,
            command_sender: tx,
            inited: false,
            playing: true,
        }
    }

    async fn add_data(&mut self, data: Arc<Stamped>) -> AnyResult<()> {
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

    async fn jump_to_live(&mut self) -> AnyResult<()> {
        if self.inited {
            if let Some(buffer_start) = self.buffer.start_time() {
                let runtime = self.get_runtime().unwrap_or(0);

                self.start_time.reset_to((buffer_start - runtime) as f64);
                debug!(
                    "Jumped to live: New start time: {:?}",
                    Duration::from_micros(self.start_time.value_u64()),
                );
            }
        }

        Ok(())
    }

    async fn seek(&mut self, _target_time: FrameTime) -> AnyResult<()> {
        self.jump_to_live().await?;
        Ok(())
    }

    async fn process_commands(&mut self) -> AnyResult<()> {
        if self.inited {
            if let Ok(command) = self.command_reciever.try_recv() {
                match command {
                    NeoMediaSenderCommand::Seek(dest) => {
                        debug!("Got Seek Request: {:?}", Duration::from_micros(dest));
                        self.seek(dest as FrameTime).await?;
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
            let cloned = buffer.buf.iter().cloned().collect::<Vec<_>>();
            let (preprocess, buffer) = cloned.split_at(split_idx);
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
                        return Some(runtime.useconds() as FrameTime);
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

    async fn update_starttime(&mut self) -> AnyResult<()> {
        self.start_time.update().await;
        if let (Some(buftime), Some(buffer_start)) = (self.get_buftime(), self.buffer.start_time())
        {
            self.start_time
                .set_target(self.start_time.value() - (buffer_start - buftime) as f64);
        }
        Ok(())
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
        if let Some(buftime) = self.get_buftime() {
            // debug!("Update: buftime: {}", buftime);
            while self
                .buffer
                .buf
                .front()
                .map(|data| data.time <= buftime)
                .unwrap_or(false)
            {
                match self.buffer.buf.pop_front() {
                    Some(frame) => {
                        buffers.push(frame);
                    }
                    None => break,
                }
            }
        }

        // collect certain frames
        self.send_buffers(&buffers).await?;

        Ok(())
    }

    async fn send_buffers<T: AsRef<Stamped>>(&mut self, medias: &[T]) -> AnyResult<()> {
        if medias.is_empty() {
            return Ok(());
        }
        if self.inited && self.playing {
            let mut vid_buffers: Vec<(FrameTime, Vec<u8>)> = vec![];
            let mut aud_buffers: Vec<(FrameTime, Vec<u8>)> = vec![];
            for media in medias.iter().map(|t| t.as_ref()) {
                let buffer = match &media.data {
                    BcMedia::Iframe(_) | BcMedia::Pframe(_) => Some(&mut vid_buffers),
                    BcMedia::Aac(_) | BcMedia::Adpcm(_) => Some(&mut aud_buffers),
                    _ => None,
                };
                let data = match &media.data {
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
            if !vid_buffers.is_empty() {
                if let Some(appsrc) = self.vid.clone() {
                    let buffers = {
                        let mut buffers = gstreamer::BufferList::new_sized(vid_buffers.len());
                        {
                            let buffers_ref = buffers.get_mut().unwrap();
                            for (time, buf) in vid_buffers.drain(..) {
                                let runtime = self.buftime_to_runtime(time);

                                let gst_buf = {
                                    let mut gst_buf =
                                        gstreamer::Buffer::with_size(buf.len()).unwrap();
                                    {
                                        let gst_buf_mut = gst_buf.get_mut().unwrap();

                                        let time =
                                            ClockTime::from_useconds(runtime.try_into().unwrap());
                                        gst_buf_mut.set_dts(time);
                                        let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
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

            if !aud_buffers.is_empty() {
                if let Some(appsrc) = self.aud.clone() {
                    let buffers = {
                        let mut buffers = gstreamer::BufferList::new_sized(aud_buffers.len());
                        {
                            let buffers_ref = buffers.get_mut().unwrap();
                            for (time, buf) in aud_buffers.drain(..) {
                                let runtime = self.buftime_to_runtime(time);

                                let gst_buf = {
                                    let mut gst_buf =
                                        gstreamer::Buffer::with_size(buf.len()).unwrap();
                                    {
                                        let gst_buf_mut = gst_buf.get_mut().unwrap();

                                        let time =
                                            ClockTime::from_useconds(runtime.try_into().unwrap());
                                        gst_buf_mut.set_dts(time);
                                        let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
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
        }
        Ok(())
    }
}

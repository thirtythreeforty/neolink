//! The component that handles passing BcMedia into
//! gstreamer media stream
use anyhow::anyhow;
use futures::stream::StreamExt;
use gstreamer::{prelude::*, ClockTime};
use gstreamer_app::AppSrc;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use log::*;
use neolink_core::bcmedia::model::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::{
    collections::{
        VecDeque,
        {hash_map::Entry, HashMap},
    },
    convert::TryInto,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    time::{Duration, Instant},
};
use tokio_stream::wrappers::ReceiverStream;

use super::{shared::*, AnyResult};

type FrameTime = i64;

const SECS: FrameTime = 1000000;
const MILLIS: FrameTime = 1000;
// const MICROS: FrameTime = 1;

const LATENCY: FrameTime = 20 * MILLIS;
const BUFFER_TIME: FrameTime = 10 * SECS;
const BUFFER_SIZE: usize = 3;

#[derive(Default)]
struct NeoBuffer {
    buf: VecDeque<Vec<BcMedia>>,
}

impl NeoBuffer {
    fn push(&mut self, item: BcMedia) {
        let mut was_iframe = false;
        match item {
            BcMedia::Iframe(data) => {
                debug!(
                    "Pushed iFrame to buffer at {:?}",
                    Duration::from_micros(data.microseconds.try_into().unwrap())
                );
                was_iframe = true;
                self.buf.push_back(vec![BcMedia::Iframe(data)])
            }
            pframe @ BcMedia::Pframe(_) => {
                if let Some(last) = self.buf.back_mut().as_mut() {
                    last.push(pframe);
                }
            }
            aac @ BcMedia::Aac(_) => {
                if let Some(last) = self.buf.back_mut().as_mut() {
                    last.push(aac);
                }
            }
            adpcm @ BcMedia::Adpcm(_) => {
                if let Some(last) = self.buf.back_mut().as_mut() {
                    last.push(adpcm);
                }
            }
            BcMedia::InfoV1(_) | BcMedia::InfoV2(_) => {}
        }
        if was_iframe {
            if let Some(last_frame_time) = self.last_iframe_time() {
                let mut time_delta = self
                    .first_iframe_time()
                    .map(|ff| last_frame_time.saturating_sub(ff));
                while let Some(time) = time_delta {
                    if time > BUFFER_TIME && self.buf.len() > BUFFER_SIZE {
                        let _ = self.buf.pop_front();
                        debug!(
                            "Popping frame with time difference of {:?}",
                            Duration::from_micros(time.try_into().unwrap_or(0))
                        );
                        time_delta = self
                            .first_iframe_time()
                            .map(|ff| last_frame_time.saturating_sub(ff));
                    } else {
                        debug!("Iframes left in the buffer: {}", self.buf.len());
                        break;
                    }
                }
            }
        }
    }

    fn last_iframe_time(&self) -> Option<FrameTime> {
        self.buf
            .back()
            .as_ref()
            .and_then(|b| b.first())
            .and_then(|f| {
                if let BcMedia::Iframe(frame) = f {
                    Some(frame.microseconds as FrameTime)
                } else {
                    None
                }
            })
    }

    fn first_iframe_time(&self) -> Option<FrameTime> {
        self.buf
            .front()
            .as_ref()
            .and_then(|b| b.first())
            .and_then(|f| {
                if let BcMedia::Iframe(frame) = f {
                    Some(frame.microseconds as FrameTime)
                } else {
                    None
                }
            })
    }

    fn start_time(&self) -> Option<FrameTime> {
        if let Some(BcMedia::Iframe(data)) =
            self.buf.front().and_then(|inner_buf| inner_buf.first())
        {
            Some(data.microseconds as FrameTime)
        } else {
            None
        }
    }

    fn end_time(&self) -> Option<FrameTime> {
        if let Some(innerbuffer) = self.buf.back() {
            innerbuffer
                .iter()
                .flat_map(|frame| match frame {
                    BcMedia::Iframe(data) => Some(data.microseconds as FrameTime),
                    BcMedia::Pframe(data) => Some(data.microseconds as FrameTime),
                    _ => None,
                })
                .max()
        } else {
            None
        }
    }
}

pub(super) struct NeoMediaSenders {
    data_source: ReceiverStream<BcMedia>,
    client_source: ReceiverStream<NeoMediaSender>,
    shared: Arc<NeoMediaShared>,
    uid: AtomicU64,
    client_data: HashMap<u64, NeoMediaSender>,
    buffer: NeoBuffer,
}

impl NeoMediaSenders {
    pub(super) fn new(
        shared: Arc<NeoMediaShared>,
        data_source: ReceiverStream<BcMedia>,
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
        match &data {
            BcMedia::Iframe(data) => {
                let frame_ms = data.microseconds as FrameTime;
                debug!(
                    "New IFrame for buffer @ {:?}",
                    Duration::from_micros(frame_ms.try_into().unwrap_or(0))
                );
                for client in self.client_data.values() {
                    debug!(
                        "  - {:?}",
                        Duration::from_micros(
                            client.buftime_to_runtime(frame_ms).try_into().unwrap_or(0)
                        )
                    );
                }
            }
            BcMedia::Pframe(data) => {
                let frame_ms = data.microseconds as FrameTime;
                debug!(
                    "New PFrame for buffer @ {:?}",
                    Duration::from_micros(frame_ms.try_into().unwrap_or(0))
                );
                for client in self.client_data.values() {
                    debug!(
                        "  - {:?}",
                        Duration::from_micros(
                            client.buftime_to_runtime(frame_ms).try_into().unwrap_or(0)
                        )
                    );
                }
            }
            _ => {}
        }
        self.buffer.push(data);
        if self.buffer.buf.len() >= BUFFER_SIZE {
            self.shared.buffer_ready.store(true, Ordering::Relaxed);
        }

        Ok(())
    }

    async fn handle_buffer(&mut self) -> AnyResult<()> {
        for key in self.client_data.keys().copied().collect::<Vec<_>>() {
            tokio::task::yield_now().await;
            // debug!("  - Client: {}", key);
            match self.client_data.entry(key) {
                Entry::Occupied(mut occ) => {
                    if let Err(e) = occ.get_mut().initialise(&self.buffer).await {
                        debug!("Could not init client: {:?}", e);
                        occ.remove();
                        continue;
                    }
                    if let Err(e) = occ.get_mut().process_commands(&self.buffer).await {
                        debug!("Could not process client command: {:?}", e);
                        occ.remove();
                        continue;
                    }
                    if let Err(e) = occ.get_mut().process_jump_to_live(&self.buffer).await {
                        debug!("Could not handle jump to live: {:?}", e);
                        occ.remove();
                        continue;
                    }
                    if let Err(e) = occ.get_mut().stretch_live(&self.buffer).await {
                        debug!("Could not sretch live: {:?}", e);
                        occ.remove();
                        continue;
                    }
                    if let Err(e) = occ.get_mut().process_buffer(&self.buffer).await {
                        debug!("Could not send client data: {:?}", e);
                        occ.remove();
                        continue;
                    }
                }
                Entry::Vacant(_) => {}
            }
        }
        Ok(())
    }

    pub(super) async fn run(&mut self) -> AnyResult<()> {
        let mut interval = tokio::time::interval(Duration::from_millis(20)); // 50 FPS
        loop {
            tokio::task::yield_now().await;
            tokio::select! {
                _ = interval.tick() => {
                    self.handle_buffer().await
                },
                Some(v) = self.data_source.next() => {
                    self.handle_new_data(v).await
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
    start_time: FrameTime,
    target_start_time: FrameTime,
    smoothed_start_time: f64,
    start_time_v: f64,
    start_time_update: Instant,
    last_sent_time: FrameTime,
    vid: Option<AppSrc>,
    aud: Option<AppSrc>,
    command_reciever: Receiver<NeoMediaSenderCommand>,
    command_sender: Sender<NeoMediaSenderCommand>,
    inited: bool,
    playing: bool,
    prebuffered: FrameTime,
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
            start_time: 0,
            target_start_time: 0,
            smoothed_start_time: 0.0,
            start_time_v: 0.0,
            start_time_update: Instant::now(),
            last_sent_time: 0,
            vid: None,
            aud: None,
            command_reciever: rx,
            command_sender: tx,
            inited: false,
            playing: true,
            prebuffered: 0,
        }
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

    async fn target_live_time(&self, buffer: &NeoBuffer) -> AnyResult<Option<FrameTime>> {
        let (fronts, backs) = buffer.buf.as_slices();
        let frames = fronts.iter().chain(backs.iter()).collect::<Vec<_>>();
        let jump_method = JumpMethod::BufferPerunit(0.75);
        Ok(match jump_method {
            JumpMethod::MiddleIFrame => {
                let idx = frames.len().saturating_div(2);
                frames.get(idx).and_then(|f| f.first()).and_then(|f| {
                    if let BcMedia::Iframe(data) = f {
                        Some(data.microseconds as FrameTime)
                    } else {
                        None
                    }
                })
            }
            JumpMethod::BufferPerunit(perunit) => {
                if let (Some(st), Some(et)) = (buffer.start_time(), buffer.end_time()) {
                    Some(((et - st) as f32 * perunit) as FrameTime + st)
                } else {
                    None
                }
            }
        })
    }

    async fn jump_to_live(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
        if self.inited {
            let runtime = self.get_runtime().unwrap_or(0);
            let target_time = self.target_live_time(buffer).await?;
            if let Some(target_time) = target_time {
                if let Some(et) = buffer.end_time() {
                    if let Ok(delta) = TryInto::<u64>::try_into(et - target_time) {
                        debug!("Expected latency: {:?}", Duration::from_micros(delta));
                    }
                }

                self.start_time = target_time - runtime;
                self.target_start_time = self.start_time;
                self.smoothed_start_time = self.start_time as f64;
                self.start_time_v = 0.0;
                self.last_sent_time = target_time;
                debug!(
                    "Target time: {:?}, New start time: {:?}, New Runtime: {:?}, Actual Runtime: {:?}",
                    Duration::from_micros(target_time.try_into().unwrap_or(0)), Duration::from_micros(self.start_time.try_into().unwrap_or(0)), Duration::from_micros(self.buftime_to_runtime(target_time).try_into().unwrap_or(0)), Duration::from_micros(runtime.try_into().unwrap_or(0))
                );
            }
        }

        Ok(())
    }

    async fn stretch_live(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
        // Desired live time
        let target_time = self.target_live_time(buffer).await?;
        // Actual live time
        let actual_time = self.get_buftime();
        let st = buffer.start_time();
        let et = buffer.end_time();
        if let (Some(target_time), Some(actual_time), Some(st), Some(et)) =
            (target_time, actual_time, st, et)
        {
            if actual_time > st.saturating_sub(LATENCY * 2)
                && actual_time < et.saturating_add(LATENCY * 2)
            {
                // Only do this while inside the buffer
                self.target_start_time = self.start_time + (target_time - actual_time); // Adjust

                // Now for the spring
                let dt = (Instant::now() - self.start_time_update)
                    .min(Duration::from_millis(200))
                    .as_secs_f64();
                spring_update(
                    &mut self.smoothed_start_time,
                    self.target_start_time as f64,
                    &mut self.start_time_v,
                    2.5,
                    dt,
                );
                if let Some(new_actual_time) = self.get_buftime() {
                    let expected_position = (new_actual_time - st) as f32 / (et - st) as f32;
                    debug!("expected_position: {}", expected_position);
                }
            }
        }
        self.start_time_update = Instant::now();
        Ok(())
    }

    async fn seek(&mut self, target_time: FrameTime) -> AnyResult<()> {
        self.last_sent_time = self.runtime_to_buftime(target_time);
        debug!(
            "Seeked last_sent_time to {:?} ({:?})",
            Duration::from_micros(self.last_sent_time.try_into().unwrap_or(0)),
            Duration::from_micros(target_time.try_into().unwrap_or(0))
        );
        Ok(())
    }

    async fn process_commands(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
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
                            self.jump_to_live(buffer).await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn initialise(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
        if !self.inited {
            if let (Some(first_ms), Some(end_ms)) = (buffer.start_time(), buffer.end_time()) {
                if end_ms.saturating_sub(first_ms) > LATENCY * 5 {
                    // Minimum buffer
                    self.inited = true;

                    self.jump_to_live(buffer).await?;

                    let _ = self.process_buffer(buffer).await; // Ignore errors let them be handled later in the loop
                }
            }
        }
        Ok(())
    }

    fn runtime_to_buftime(&self, runtime: FrameTime) -> FrameTime {
        runtime.saturating_add(self.start_time)
    }

    fn buftime_to_runtime(&self, buftime: FrameTime) -> FrameTime {
        buftime.saturating_sub(self.start_time).max(0)
    }

    async fn process_buffer(&mut self, buf: &NeoBuffer) -> AnyResult<()> {
        if self.inited && self.playing {
            let runtime = self.get_runtime();
            if let Some(runtime) = runtime {
                // We are live only send the buffer up to the runtime
                let min_time = self.last_sent_time;
                let max_time = self.runtime_to_buftime(runtime);
                self.last_sent_time = self.send_buffer_between(buf, min_time, max_time).await?;
            } else {
                // We are not playing send pre buffers so that the elements can init themselves
                let min_time = self.prebuffered;
                let max_time = self.last_sent_time;
                self.prebuffered = self.send_buffer_between(buf, min_time, max_time).await?;
            }
        }
        Ok(())
    }

    async fn send_buffer_between(
        &mut self,
        buf: &NeoBuffer,
        min_time: FrameTime,
        max_time: FrameTime,
    ) -> AnyResult<FrameTime> {
        if let (Some(buftime), Some(start_time), Some(end_time)) =
            (self.get_buftime(), buf.start_time(), buf.end_time())
        {
            debug!(
                "Buffer Run Position: {}",
                (buftime - start_time) as f32 / (end_time - start_time) as f32
            );
        }
        let mut last_sent_time = min_time;
        let mut found_start = false;
        let mut buf_it = buf.buf.iter().peekable();
        while let Some(frames) = buf_it.next() {
            tokio::task::yield_now().await;
            if !found_start {
                let next_frames = buf_it.peek();
                if let Some(BcMedia::Iframe(frame)) = next_frames.and_then(|b| b.first()) {
                    // Get time of next IFrame
                    // if it is after min_time, then the
                    // start shuld happen between frames and next_frames
                    if frame.microseconds as FrameTime > min_time {
                        found_start = true;
                    }
                }
            }

            if found_start {
                // We have found the start send eveythin until we get passed the
                // max time
                for frame in frames {
                    let frame_time = match frame {
                        BcMedia::Iframe(data) => data.microseconds as FrameTime,
                        BcMedia::Pframe(data) => data.microseconds as FrameTime,
                        _ => last_sent_time,
                    };
                    if frame_time > min_time && frame_time <= max_time {
                        if let (Some(start_time), Some(end_time)) =
                            (buf.start_time(), buf.end_time())
                        {
                            debug!(
                                "Frame Run Position: {}",
                                (frame_time - start_time) as f32 / (end_time - start_time) as f32
                            );
                        }
                        self.send_buffer(frame).await?;
                        last_sent_time = frame_time;
                    } else if frame_time > max_time {
                        return Ok(last_sent_time);
                    }
                }
            }
        }
        Ok(last_sent_time)
    }

    async fn process_jump_to_live(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
        if self.inited {
            if let (Some(buffer_start), Some(buffer_end)) = (buffer.start_time(), buffer.end_time())
            {
                let runtime = self.get_buftime().unwrap_or(self.last_sent_time);
                if runtime < buffer_start.saturating_sub(15 * SECS)
                    || runtime > buffer_end.saturating_add(15 * SECS)
                {
                    debug!(
                        "Outside buffer jumping to live: {:?} < {:?} < {:?}",
                        Duration::from_micros(buffer_start.try_into().unwrap_or(0)),
                        Duration::from_micros(runtime.try_into().unwrap_or(0)),
                        Duration::from_micros(buffer_end.try_into().unwrap_or(0))
                    );
                    self.jump_to_live(buffer).await?;
                }
            }
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

    async fn send_buffer(&mut self, media: &BcMedia) -> AnyResult<bool> {
        if self.inited && self.playing {
            let buftime = match media {
                BcMedia::Iframe(data) => data.microseconds as FrameTime,
                BcMedia::Pframe(data) => data.microseconds as FrameTime,
                _ => self.last_sent_time,
            };
            let runtime = self.buftime_to_runtime(buftime);

            let buf = match media {
                BcMedia::Iframe(data) => Some(&data.data),
                BcMedia::Pframe(data) => Some(&data.data),
                BcMedia::Aac(data) => Some(&data.data),
                BcMedia::Adpcm(data) => Some(&data.data),
                _ => None,
            };
            let appsrc = match media {
                BcMedia::Iframe(_) | BcMedia::Pframe(_) => self.vid.as_ref(),
                BcMedia::Aac(_) | BcMedia::Adpcm(_) => self.aud.as_ref(),
                _ => None,
            };

            if let (Some(buf), Some(appsrc)) = (buf, appsrc) {
                debug!(
                    "DTS: {:?}, Expected: {:?}, Position in Buffer",
                    Duration::from_micros(runtime.try_into().unwrap_or(0)),
                    self.get_runtime()
                        .map(|t| Duration::from_micros(t.try_into().unwrap_or(0)))
                );

                let mut gst_buf = gstreamer::Buffer::with_size(buf.len()).unwrap();
                {
                    let gst_buf_mut = gst_buf.get_mut().unwrap();

                    let time = ClockTime::from_useconds(runtime.try_into().unwrap());
                    gst_buf_mut.set_dts(time);
                    let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
                    gst_buf_data.copy_from_slice(buf);
                }
                // debug!("Buffer pushed");
                let thread_appsrc = appsrc.clone(); // GObjects are refcounted
                let res = tokio::task::spawn_blocking(move || {
                    thread_appsrc
                        .push_buffer(gst_buf.copy())
                        .map(|_| ())
                        .map_err(|_| anyhow!("Could not push buffer to appsrc"))
                })
                .await;
                match &res {
                    Err(e) => {
                        debug!("Failed to send buffer: {:?}", e);
                    }
                    Ok(Err(e)) => {
                        debug!("Failed to send buffer: {:?}", e);
                    }
                    Ok(Ok(_)) => {}
                };
                res??;
            }
        }
        Ok(true)
    }
}

const EPS: f64 = 1e-5;
fn halflife_to_damping(halflife: f64) -> f64 {
    (4.0 * std::f64::consts::LN_2) / (halflife + EPS)
}

// fn damping_to_halflife(damping: f32) -> f32 {
//     (4.0f32 * std::f32::consts::LN_2) / (damping + EPS)
// }
fn fast_negexp(x: f64) -> f64 {
    1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x)
}

fn spring_update(value: &mut f64, target: f64, velocity: &mut f64, halflife: f64, dt: f64) {
    let y = halflife_to_damping(halflife) / 2.0;
    let j0 = *value - target;
    let j1 = *velocity + j0 * y;
    let eydt = fast_negexp(y * dt);

    *value = eydt * (j0 + j1 * dt) + target;
    *velocity = eydt * (*velocity - j1 * y * dt);
}

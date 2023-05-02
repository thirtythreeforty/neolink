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
// use tokio::time::{interval, Duration, MissedTickBehavior};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    time::Duration,
};
use tokio_stream::wrappers::ReceiverStream;

use super::{shared::*, AnyResult};

const LATENCY: Duration = Duration::from_millis(20);
const BUFFER_TIME: Duration = Duration::from_secs(10);
const BUFFER_SIZE: usize = 2;

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
                        debug!("Popping frame with time difference of {:?}", time);
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

    fn last_iframe_time(&self) -> Option<Duration> {
        self.buf
            .back()
            .as_ref()
            .and_then(|b| b.first())
            .and_then(|f| {
                if let BcMedia::Iframe(frame) = f {
                    Some(Duration::from_micros(
                        frame.microseconds.try_into().unwrap(),
                    ))
                } else {
                    None
                }
            })
    }

    fn first_iframe_time(&self) -> Option<Duration> {
        self.buf
            .front()
            .as_ref()
            .and_then(|b| b.first())
            .and_then(|f| {
                if let BcMedia::Iframe(frame) = f {
                    Some(Duration::from_micros(
                        frame.microseconds.try_into().unwrap(),
                    ))
                } else {
                    None
                }
            })
    }

    fn start_time(&self) -> Option<Duration> {
        if let Some(BcMedia::Iframe(data)) =
            self.buf.front().and_then(|inner_buf| inner_buf.first())
        {
            Some(Duration::from_micros(data.microseconds.try_into().unwrap()))
        } else {
            None
        }
    }

    fn end_time(&self) -> Option<Duration> {
        if let Some(innerbuffer) = self.buf.back() {
            innerbuffer
                .iter()
                .flat_map(|frame| match frame {
                    BcMedia::Iframe(data) => {
                        Some(Duration::from_micros(data.microseconds.try_into().unwrap()))
                    }
                    BcMedia::Pframe(data) => {
                        Some(Duration::from_micros(data.microseconds.try_into().unwrap()))
                    }
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
        client.initialise(&self.buffer).await?;
        self.client_data
            .insert(self.uid.fetch_add(1, Ordering::Relaxed), client);
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
        self.handle_buffer().await?;
        match &data {
            BcMedia::Iframe(data) => {
                let frame_ms = Duration::from_micros(data.microseconds.try_into().unwrap());
                debug!("New IFrame for buffer @ {:?}", frame_ms);
                for client in self.client_data.values() {
                    debug!("  - {:?}", client.buftime_to_runtime(frame_ms));
                }
            }
            BcMedia::Pframe(data) => {
                let frame_ms = Duration::from_micros(data.microseconds.try_into().unwrap());
                debug!("New PFrame for buffer @ {:?}", frame_ms);
                for client in self.client_data.values() {
                    debug!("  - {:?}", client.buftime_to_runtime(frame_ms));
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
        let mut interval = tokio::time::interval(LATENCY / 3);
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
    start_time: i128,
    last_sent_time: Duration,
    vid: Option<AppSrc>,
    aud: Option<AppSrc>,
    command_reciever: Receiver<NeoMediaSenderCommand>,
    command_sender: Sender<NeoMediaSenderCommand>,
    inited: bool,
    playing: bool,
    prebuffered: Duration,
}

impl NeoMediaSender {
    pub(super) fn new() -> Self {
        let (tx, rx) = channel(30);
        Self {
            start_time: 0,
            last_sent_time: Duration::ZERO,
            vid: None,
            aud: None,
            command_reciever: rx,
            command_sender: tx,
            inited: false,
            playing: true,
            prebuffered: Duration::ZERO,
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

    async fn jump_to_live(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
        if self.inited {
            let runtime = self.get_runtime().unwrap_or(Duration::ZERO);
            let (fronts, backs) = buffer.buf.as_slices();
            let frames = fronts.iter().chain(backs.iter()).collect::<Vec<_>>();
            let idx = frames.len().saturating_div(2);
            let target_time = frames.get(idx).and_then(|f| f.first()).and_then(|f| {
                if let BcMedia::Iframe(data) = f {
                    Some(Duration::from_micros(data.microseconds.try_into().unwrap()))
                } else {
                    None
                }
            });
            if let Some(target_time) = target_time {
                self.start_time = TryInto::<i128>::try_into(target_time.as_micros()).unwrap()
                    - TryInto::<i128>::try_into(runtime.as_micros()).unwrap();
                self.last_sent_time = target_time;
                debug!(
                    "Target time: {:?}, New start time: {:?}, New Runtime: {:?}, Actual Runtime: {:?}",
                    target_time, self.start_time, self.buftime_to_runtime(target_time), runtime
                );
            }
        }

        Ok(())
    }

    async fn seek(&mut self, target_time: Duration) -> AnyResult<()> {
        self.last_sent_time = self.runtime_to_buftime(target_time);
        debug!(
            "Seeked last_sent_time to {:?} ({:?})",
            self.last_sent_time, target_time
        );
        Ok(())
    }

    async fn process_commands(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
        if self.inited {
            if let Ok(command) = self.command_reciever.try_recv() {
                match command {
                    NeoMediaSenderCommand::Seek(dest) => {
                        debug!("Got Seek Request: {}", dest);
                        self.seek(Duration::from_micros(dest)).await?;
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

                    self.process_buffer(buffer).await?;
                }
            }
        }
        Ok(())
    }

    fn runtime_to_buftime(&self, runtime: Duration) -> Duration {
        Duration::from_micros(
            TryInto::<u64>::try_into(
                TryInto::<i128>::try_into(runtime.as_micros())
                    .unwrap()
                    .saturating_add(self.start_time),
            )
            .unwrap_or(0),
        )
    }

    fn buftime_to_runtime(&self, buftime: Duration) -> Duration {
        Duration::from_micros(
            TryInto::<u64>::try_into(
                TryInto::<i128>::try_into(buftime.as_micros())
                    .unwrap()
                    .saturating_sub(self.start_time),
            )
            .unwrap_or(0),
        )
    }

    async fn process_buffer(&mut self, buf: &NeoBuffer) -> AnyResult<()> {
        if self.inited && self.playing {
            let runtime = self.get_runtime();
            if let Some(runtime) = runtime {
                // We are live only send the buffer up to the runtime
                let min_time = self.last_sent_time;
                let max_time = self.runtime_to_buftime(runtime).saturating_add(LATENCY);
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
        min_time: Duration,
        max_time: Duration,
    ) -> AnyResult<Duration> {
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
                    if Duration::from_micros(frame.microseconds.try_into().unwrap()) > min_time {
                        found_start = true;
                    }
                }
            }

            if found_start {
                // We have found the start send eveythin until we get passed the
                // max time
                for frame in frames {
                    let frame_time = match frame {
                        BcMedia::Iframe(data) => {
                            Duration::from_micros(data.microseconds.try_into().unwrap())
                        }
                        BcMedia::Pframe(data) => {
                            Duration::from_micros(data.microseconds.try_into().unwrap())
                        }
                        _ => last_sent_time,
                    };
                    if frame_time > min_time && frame_time <= max_time {
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
                if runtime < buffer_start.saturating_sub(LATENCY * 2)
                    || runtime > buffer_end.saturating_add(LATENCY * 2)
                {
                    debug!(
                        "Outside buffer jumping to live: {:?} < {:?} < {:?}",
                        buffer_start, runtime, buffer_end
                    );
                    self.jump_to_live(buffer).await?;
                }
            }
        }
        Ok(())
    }

    fn get_runtime(&self) -> Option<Duration> {
        if let Some(appsrc) = self.vid.as_ref() {
            if let Some(clock) = appsrc.clock() {
                if let Some(time) = clock.time() {
                    if let Some(base_time) = appsrc.base_time() {
                        let runtime = time.saturating_sub(base_time);
                        return Some(Duration::from_nanos(runtime.nseconds()));
                    }
                }
            }
        }
        None
    }

    fn get_buftime(&self) -> Option<Duration> {
        if let Some(appsrc) = self.vid.as_ref() {
            if let Some(clock) = appsrc.clock() {
                if let Some(time) = clock.time() {
                    if let Some(base_time) = appsrc.base_time() {
                        let runtime = time.saturating_sub(base_time);
                        return Some(
                            self.runtime_to_buftime(Duration::from_nanos(runtime.nseconds())),
                        );
                    }
                }
            }
        }
        None
    }

    async fn send_buffer(&mut self, media: &BcMedia) -> AnyResult<bool> {
        if self.inited && self.playing {
            let buftime = match media {
                BcMedia::Iframe(data) => {
                    Duration::from_micros(data.microseconds.try_into().unwrap())
                }
                BcMedia::Pframe(data) => {
                    Duration::from_micros(data.microseconds.try_into().unwrap())
                }
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
                debug!("DTS: {:?}, Expected: {:?}", runtime, self.get_runtime());

                let mut gst_buf = gstreamer::Buffer::with_size(buf.len()).unwrap();
                {
                    let gst_buf_mut = gst_buf.get_mut().unwrap();

                    let time = ClockTime::from_useconds(runtime.as_micros().try_into().unwrap());
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

//! The component that handles passing BcMedia into
//! gstreamer media stream
use anyhow::anyhow;
use futures::stream::StreamExt;
use gstreamer::{prelude::*, ClockTime};
use gstreamer_app::AppSrc;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use log::*;
use neolink_core::bcmedia::model::*;
use std::collections::{
    VecDeque,
    {hash_map::Entry, HashMap},
};
use std::{
    iter::Iterator,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
// use tokio::time::{interval, Duration, MissedTickBehavior};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    time::Duration,
};
use tokio_stream::wrappers::ReceiverStream;

use super::{shared::*, AnyResult};

const LATENCY: Duration = Duration::from_micros(200);

#[derive(Default)]
struct NeoBuffer {
    buf: VecDeque<Vec<BcMedia>>,
}

impl NeoBuffer {
    fn push(&mut self, item: BcMedia) {
        match item {
            iframe @ BcMedia::Iframe(_) => self.buf.push_back(vec![iframe]),
            pframe @ BcMedia::Pframe(_) => {
                if let Some(last) = self.buf.make_contiguous().last_mut().as_mut() {
                    last.push(pframe);
                }
            }
            aac @ BcMedia::Aac(_) => {
                if let Some(last) = self.buf.make_contiguous().last_mut().as_mut() {
                    last.push(aac);
                }
            }
            adpcm @ BcMedia::Adpcm(_) => {
                if let Some(last) = self.buf.make_contiguous().last_mut().as_mut() {
                    last.push(adpcm);
                }
            }
            BcMedia::InfoV1(_) | BcMedia::InfoV2(_) => {}
        }
        while self.buf.len() > 25 {
            // 25 iframes
            let _ = self.buf.pop_front();
        }
    }

    fn start_time(&self) -> Option<Duration> {
        if let Some(BcMedia::Iframe(data)) =
            self.buf.front().and_then(|inner_buf| inner_buf.first())
        {
            Some(Duration::from_micros(data.microseconds))
        } else {
            None
        }
    }

    fn end_time(&self) -> Option<Duration> {
        if let Some(innerbuffer) = self.buf.back() {
            let mut last_ms = None;
            for frame in innerbuffer.iter() {
                match frame {
                    BcMedia::Iframe(frame) => {
                        let frame_ms = Duration::from_micros(frame.microseconds);
                        let v = last_ms.get_or_insert(frame_ms);
                        
                        if *v < frame.microseconds {
                            *v = frame.microseconds;
                        }
                    }
                    BcMedia::Pframe(frame) => {
                        let frame_ms = Duration::from_micros(frame.microseconds);
                        let v = last_ms.get_or_insert(frame_ms);
                        
                        if *v < frame_ms {
                            *v = frame_ms;
                        }
                    }
                    _ => {}
                }
            }
            last_ms
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
                        AudFormats::Adpcm(data.data.len() as u16);
                }
            }
            _ => {}
        }
    }

    async fn handle_new_data(&mut self, data: BcMedia) -> AnyResult<()> {
        // debug!("Handle new data");
        self.update_mediatypes(&data).await;
        for key in self.client_data.keys().copied().collect::<Vec<_>>() {
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
        self.buffer.push(data);

        Ok(())
    }

    pub(super) async fn run(&mut self) -> AnyResult<()> {
        loop {
            tokio::select! {
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
    start_time: Duration,
    last_sent_time: Durataion,
    vid: Option<AppSrc>,
    aud: Option<AppSrc>,
    command_reciever: Receiver<NeoMediaSenderCommand>,
    command_sender: Sender<NeoMediaSenderCommand>,
    inited: bool,
    playing: bool,
}

impl NeoMediaSender {
    pub(super) fn new() -> Self {
        let (tx, rx) = channel(3);
        Self {
            start_time: Duration::ZERO,
            last_sent_time: Duration:ZERO,
            vid: None,
            aud: None,
            command_reciever: rx,
            command_sender: tx,
            inited: false,
            playing: true,
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
            if let (Some(runtime), Some(buffer_start), Some(buffer_end)) = (self.get_runtime(), buffer.start_time(), buffer.end_time()) {
                // Jump to the mid point
                let target_time = (buffer_start + buffer_end) / 2;
                self.start_time = target_time - runtime;
                self.last_sent_time = target_time;
            }
        }
    }
    
    async fn seek(&mut self, target_time: Duration) -> AnyResult<()> {
        self.last_sent_time = runtime_to_buftime(target_time);
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
                // if let Some(source) = self.vid.as_ref() {
                //     source.set_base_time(ClockTime::from_mseconds(0));
                // }
                // if let Some(source) = self.aud.as_ref() {
                //     source.set_base_time(ClockTime::from_mseconds(0));
                // }

                self.start_time = (first_ms + end_ms) / 2;

                self.inited = true;

                self.process_buffer(buffer).await?;
            }
        }
        Ok(())
    }
    
    fn runtime_to_buftime(&self, runtime: Duration) -> Duration {
        runtime + self.start_time
    }
    
    fn buftime_to_runtime(&self, buftime: Duration) -> Duration {
        if buftime > self.start_time {
            buftime - self.start_time
        } else {
            0
        }
    }
    
    

    async fn process_buffer(&mut self, buf: &NeoBuffer) -> AnyResult<()> {
        if self.inited && self.playing {
            let runtime = self.get_runtime();
            if let Some(runtime) = runtime {
                // We are live only send the buffer up to the runtime
                let min_time = self.last_sent_time;
                let max_time = self.runtime_to_buftime(runtime) + LATENCY;
                
                let mut found_start = false;
                let mut buf_it = buf.buf.iter().peekable();
                while let Some(frames) = buf_it.next() {
                    if !found_start {
                        let next_frames = buf_it.peek();
                        if let Some(BcMedia::Iframe(frame)) = next_frames.first() {
                            // Get time of next IFrame
                            // if it is after min_time, then the
                            // start shuld happen between frames and next_frames
                            if Duration::from_micros(frame.microseconds) > min_time {
                                found_start = true;
                            }
                        }
                    }
                    
                    if found_start {
                        // We have found the start send eveythin until we get passed the
                        // max time
                        for frame in frames {
                            let frame_time = match frame {
                                BcMedia::Iframe(data) => Duration::from_micros(data.microseconds),
                                BcMedia::Pframe(data) => Duration::from_micros(data.microseconds),
                                _ => self.last_sent_time,
                            };
                            if frame_time > min_time && frame_time <= max_time {
                                self.send_buffer(frame).await?;
                                self.last_sent_time = frame_time;
                            } else if frame_time > max_time {
                                return Ok(());
                            }
                        }
                    }
                }
                
            } else {
                // Not live. Send the WHOLE buffer for analysis
                for frame in buf.iter() {
                    self.send_buffer(frame).await?;
                }
            }
        }

        Ok(())
    }

    async fn process_jump_to_live(&mut self, buffer: &NeoBuffer) -> AnyResult<()> {
        if self.inited {
            if let (Some(runtime), Some(buffer_start), Some(buffer_end)) = (self.get_runtime(), buffer.start_time(), buffer.end_time()) {
                if runtime < (buffer_start - LATENCY) || runtime > (buffer_end + LATENCY) {
                    self.jump_to_live(buffer)?;
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
                        let runtime = time - base_time;
                        return Some(Duration::from_nanos(runtime.nseconds()));
                    }
                }
            }
        }
        None
    }

    async fn send_buffer(&mut self, media: &BcMedia) -> AnyResult<bool> {
        if self.inited && self.playing {
            let buftime = match media {
                BcMedia::Iframe(data) => Duration::from_micros(data.microseconds),
                BcMedia::Pframe(data) => Duration::from_micros(data.microseconds),
                _ => self.last_sent_time,
            };
            let runtime = buftime_to_runtime(buftime);
            
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
                debug!("PTS: {:?}", runtime);

                let mut gst_buf = gstreamer::Buffer::with_size(buf.len()).unwrap();
                {
                    let gst_buf_mut = gst_buf.get_mut().unwrap();

                    let time = ClockTime::from_useconds(runtime.as_microseconds());
                    gst_buf_mut.set_pts(time);
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

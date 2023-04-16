//! The component that handles passing BcMedia into
//! gstreamer media stream
use anyhow::anyhow;
use futures::stream::StreamExt;
use gstreamer::ClockTime;
use gstreamer_app::AppSrc;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use log::*;
use neolink_core::bcmedia::model::*;
use std::collections::{hash_map::Entry, HashMap};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
// use tokio::time::{interval, Duration, MissedTickBehavior};
use tokio_stream::wrappers::ReceiverStream;

use super::{shared::*, AnyResult};

pub(super) struct NeoMediaSender {
    pub(super) data_source: ReceiverStream<BcMedia>,
    pub(super) clientsource: ReceiverStream<ClientPipelineData>,
    pub(super) shared: Arc<NeoMediaShared>,
    // Used to generate the key for the hashmaps
    pub(super) uid: AtomicU64,
    // Hashmap so we can alter the data in ClientData while also allowing for remove at any point
    // If you know a better collections pm me
    pub(super) clientdata: HashMap<u64, ClientPipelineData>,
    pub(super) waiting_for_iframe: bool,
}

#[derive(Clone)]
struct StampedData {
    ms: u64,
    data: Vec<u8>,
}

impl NeoMediaSender {
    pub(super) async fn run(&mut self) -> AnyResult<()> {
        // let mut resend_pause = interval(Duration::from_secs_f32(1.0f32 / 5.0f32));
        // resend_pause.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut buffer_a: Vec<StampedData> = Default::default();
        let mut buffer_b: Vec<StampedData> = Default::default();
        let buffer_read = &mut buffer_a;
        let buffer_write = &mut buffer_b;

        let mut audbuffer_a: Vec<StampedData> = Default::default();
        let mut audbuffer_b: Vec<StampedData> = Default::default();
        let audbuffer_read = &mut audbuffer_a;
        let audbuffer_write = &mut audbuffer_b;
        loop {
            self.shared
                .number_of_clients
                .store(self.clientdata.len(), Ordering::Relaxed);
            self.shared
                .buffer_ready
                .store(!buffer_read.is_empty(), Ordering::Relaxed);
            debug!("Sender: Get");
            tokio::select! {
                v = self.data_source.next() => {
                    debug!("Sender: Got Data");
                    // debug!("data_source recieved");
                    if let Some(bc_media) = v {
                        if ! self.skip_bcmedia(&bc_media)? {
                            // debug!("Not skipped");
                            // resend_pause.reset();
                            self.inspect_bcmedia(&bc_media).await?;
                            match bc_media {
                                BcMedia::Iframe(frame) => {
                                    if buffer_write.len() > 100 {
                                        std::mem::swap(buffer_read, buffer_write);
                                        buffer_write.clear();
                                        std::mem::swap(audbuffer_read, audbuffer_write);
                                        audbuffer_write.clear();
                                    }
                                    let new_data = StampedData {
                                            ms: frame.microseconds as u64,
                                            data: frame.data,
                                    };
                                    self.process_vidbuffer(&new_data).await?;
                                    buffer_write.push(new_data);
                                }
                                BcMedia::Pframe(frame) => {
                                    let new_data = StampedData {
                                            ms: frame.microseconds as u64,
                                            data: frame.data,
                                    };
                                    self.process_vidbuffer(&new_data).await?;
                                    buffer_write.push(new_data);
                                }
                                BcMedia::Aac(aac) => {
                                    if let Some(last) = buffer_write.last().as_ref() {
                                        let new_data = StampedData {
                                            ms: last.ms,
                                            data: aac.data,
                                        };
                                        self.process_audbuffer(&new_data).await?;
                                        audbuffer_write.push(new_data);
                                    }

                                }
                                BcMedia::Adpcm(adpcm) => {
                                    if let Some(last) = buffer_write.last().as_ref() {
                                        let new_data = StampedData {
                                            ms: last.ms,
                                            data: adpcm.data,
                                        };
                                        self.process_audbuffer(&new_data).await?;
                                        audbuffer_write.push(new_data);
                                    }

                                }
                                _ => {}
                            }
                        }
                    } else {
                        break;
                    }
                }
                v = self.clientsource.next() => {
                    debug!("Sender: Got Client");
                    if let Some(mut clientdata) = v {
                        // Resend the video and audio buffers
                        if !clientdata.inited {
                            clientdata.inited = true;
                            let mut vid_resend = buffer_read.iter().chain(buffer_write.iter()).enumerate();
                            let mut aud_resend = audbuffer_read.iter().chain(audbuffer_write.iter()).enumerate();
                            loop {
                                let next_vid = vid_resend.next();
                                if let Some((idx, next_vid)) = next_vid.as_ref() {
                                    if let Some(src) = clientdata.vidsrc.as_ref() {
                                        if Self::send_buffer(
                                            src,
                                            next_vid.data.as_slice(),
                                            *idx as u64,
                                            0,
                                        ).await.is_err() {
                                            break;
                                        };
                                    }
                                }
                                let next_aud = aud_resend.next();
                                if let Some((idx, next_aud)) = next_aud.as_ref() {
                                    if let Some(src) = clientdata.audsrc.as_ref() {
                                        if Self::send_buffer(
                                            src,
                                            next_aud.data.as_slice(),
                                            *idx as u64,
                                            0,
                                        ).await.is_err() {
                                            break;
                                        };
                                    }
                                }
                                if next_vid.is_none() && next_aud.is_none() {
                                    break;
                                }
                            }
                        }
                        // Save the client for future sends
                        self.clientdata.insert(self.uid.fetch_add(1, Ordering::Relaxed) , clientdata);
                    } else {
                        break;
                    }
                },
                // _ = resend_pause.tick() => {
                //     if let Some(live) = buffer.last().cloned() {
                //         self.process_vidbuffer(&ProcessData {
                //             live,
                //             resend: buffer.as_slice()
                //         })?;
                //     }
                // }
            }
        }
        Ok(())
    }

    async fn process_vidbuffer(&mut self, stamped_data: &StampedData) -> AnyResult<()> {
        for key in self
            .clientdata
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .drain(..)
        {
            match self.clientdata.entry(key) {
                Entry::Occupied(data) => {
                    if let Some(vidsrc) = data.get().vidsrc.as_ref() {
                        if Self::send_buffer(
                            vidsrc,
                            stamped_data.data.as_slice(),
                            stamped_data.ms,
                            data.get().start_time,
                        )
                        .await
                        .is_err()
                        {
                            data.remove();
                        }
                    } else {
                        data.remove();
                    }
                }
                Entry::Vacant(_) => {}
            }
        }
        Ok(())
    }

    async fn process_audbuffer(&mut self, stamped_data: &StampedData) -> AnyResult<()> {
        for key in self
            .clientdata
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .drain(..)
        {
            match self.clientdata.entry(key) {
                Entry::Occupied(data) => {
                    if let Some(audsrc) = data.get().audsrc.as_ref() {
                        if Self::send_buffer(
                            audsrc,
                            stamped_data.data.as_slice(),
                            stamped_data.ms,
                            data.get().start_time,
                        )
                        .await
                        .is_err()
                        {
                            data.remove();
                        }
                    } else {
                        data.remove();
                    }
                }
                Entry::Vacant(_) => {}
            }
        }

        Ok(())
    }

    async fn send_buffer(
        appsrc: &AppSrc,
        buf: &[u8],
        frame_ms: u64,
        start_time: u64,
    ) -> AnyResult<()> {
        let micros = if frame_ms > start_time {
            frame_ms - start_time
        } else {
            start_time
        };

        let mut gst_buf = gstreamer::Buffer::with_size(buf.len()).unwrap();
        {
            let gst_buf_mut = gst_buf.get_mut().unwrap();

            let time = ClockTime::from_useconds(micros);
            gst_buf_mut.set_pts(time);
            // debug!(
            //     "PTS set to: {:?} ({}-{}={:?})",
            //     time, frame_ms, start_time, micros
            // );
            let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
            gst_buf_data.copy_from_slice(buf);
        }
        // debug!("Buffer pushed");
        let thread_appsrc = appsrc.clone(); // GObjects are refcounted
        tokio::task::spawn_blocking(move || {
            thread_appsrc
                .push_buffer(gst_buf.copy())
                .map(|_| ())
                .map_err(|_| anyhow!("Could not push buffer to appsrc"))
        })
        .await?
    }

    fn skip_bcmedia(&mut self, bc_media: &BcMedia) -> AnyResult<bool> {
        if self.waiting_for_iframe {
            if let BcMedia::Iframe(_) = bc_media {
                self.waiting_for_iframe = false;
            } else {
                log::debug!("Skipping bcmedia");
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn inspect_bcmedia(&mut self, bc_media: &BcMedia) -> AnyResult<()> {
        let old_vid = *self.shared.vid_format.read().await;
        let old_aud = *self.shared.aud_format.read().await;
        match bc_media {
            BcMedia::Iframe(frame) => {
                match frame.video_type {
                    VideoType::H264 => {
                        (*self.shared.vid_format.write().await) = VidFormats::H264;
                    }
                    VideoType::H265 => {
                        (*self.shared.vid_format.write().await) = VidFormats::H265;
                    }
                }
                self.shared
                    .microseconds
                    .store(frame.microseconds as u64, Ordering::Relaxed);
                // log::debug!(
                //     "Time set to {}",
                //     self.shared.microseconds.load(Ordering::Relaxed)
                // );
            }
            BcMedia::Pframe(frame) => {
                match frame.video_type {
                    VideoType::H264 => {
                        (*self.shared.vid_format.write().await) = VidFormats::H264;
                    }
                    VideoType::H265 => {
                        (*self.shared.vid_format.write().await) = VidFormats::H265;
                    }
                }
                self.shared
                    .microseconds
                    .store(frame.microseconds as u64, Ordering::Relaxed);
                // log::debug!(
                //     "Time set to {}",
                //     self.shared.microseconds.load(Ordering::Relaxed)
                // );
            }
            BcMedia::Aac(_aac) => {
                (*self.shared.aud_format.write().await) = AudFormats::Aac;
            }
            BcMedia::Adpcm(adpcm) => {
                (*self.shared.aud_format.write().await) = AudFormats::Adpcm(adpcm.data.len() as u16)
            }
            _ => {}
        }
        let new_vid = *self.shared.vid_format.read().await;
        if new_vid != old_vid {
            log::debug!("Video format set to: {:?} from {:?}", new_vid, old_vid);
        }
        let new_aud = *self.shared.aud_format.read().await;
        if old_aud != new_aud {
            log::debug!("Audio format set to: {:?} from {:?}", new_aud, old_aud);
        }
        Ok(())
    }
}

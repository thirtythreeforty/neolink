//! The component that handles passing BcMedia into
//! gstreamer media stream
use anyhow::anyhow;
use futures::stream::StreamExt;
use gstreamer::ClockTime;
use gstreamer_app::AppSrc;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use neolink_core::bcmedia::model::*;
use std::collections::HashMap;
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
struct ProcessData<'a> {
    live: StampedData,
    resend: &'a [StampedData],
}

impl NeoMediaSender {
    pub(super) async fn run(&mut self) -> AnyResult<()> {
        // let mut resend_pause = interval(Duration::from_secs_f32(1.0f32 / 5.0f32));
        // resend_pause.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut buffer_a: Vec<StampedData> = Default::default();
        let mut buffer_b: Vec<StampedData> = Default::default();
        let buffer_read = &mut buffer_a;
        let buffer_write = &mut buffer_b;
        loop {
            self.shared
                .number_of_clients
                .store(self.clientdata.len(), Ordering::Relaxed);
            self.shared
                .buffer_ready
                .store(!buffer_read.is_empty(), Ordering::Relaxed);
            tokio::select! {
                v = self.data_source.next() => {
                    // debug!("data_source recieved");
                    if let Some(bc_media) = v {
                        if ! self.skip_bcmedia(&bc_media)? {
                            // debug!("Not skipped");
                            // resend_pause.reset();
                            self.inspect_bcmedia(&bc_media).await?;
                            match bc_media {
                                BcMedia::Iframe(frame) => {
                                    std::mem::swap(buffer_read, buffer_write);
                                    buffer_write.clear();
                                    buffer_write.push(
                                        StampedData {
                                            ms: frame.microseconds as u64,
                                            data: frame.data.clone(),
                                        }
                                    );
                                    self.process_vidbuffer(&ProcessData {
                                        live: StampedData{
                                            ms: frame.microseconds as u64,
                                            data: frame.data,
                                        },
                                        resend: buffer_read.as_slice()
                                    })?;
                                }
                                BcMedia::Pframe(frame) => {
                                    buffer_write.push(
                                        StampedData {
                                            ms: frame.microseconds as u64,
                                            data: frame.data.clone(),
                                        }
                                    );
                                    self.process_vidbuffer(&ProcessData {
                                        live: StampedData{
                                            ms: frame.microseconds as u64,
                                            data: frame.data,
                                        },
                                        resend: buffer_read.as_slice()
                                    })?;
                                }
                                BcMedia::Aac(aac) => {
                                    if let Some(last) = buffer_write.last().as_ref() {
                                        self.process_audbuffer(aac.data.as_slice(), last.ms)?;
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
                    if let Some(clientdata) = v {
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

    fn process_vidbuffer(&mut self, process_data: &ProcessData) -> AnyResult<()> {
        self.clientdata.retain(|_, data| {
            if let Some(vidsrc) = data.vidsrc.as_ref() {
                // If ! inited then we just joined the stream
                // Send all data from the last iframe to catch up
                // This should eliminate visual artifact that arise from recieving a pframe
                // before an iframe
                if !data.inited {
                    data.inited = true;
                    for (idx, resend_buffer) in process_data.resend.iter().enumerate() {
                        if Self::send_buffer(vidsrc, resend_buffer.data.as_slice(), idx as u64, 0)
                            .is_err()
                        {
                            return false; // If fails then this appsrc is dead
                        }
                    }
                }
                Self::send_buffer(
                    vidsrc,
                    process_data.live.data.as_slice(),
                    process_data.live.ms,
                    data.start_time,
                )
                .is_ok() // If ok retain is true
            } else {
                // Audio only
                true
            }
        });

        Ok(())
    }

    fn process_audbuffer(&mut self, buf: &[u8], frame_ms: u64) -> AnyResult<()> {
        self.clientdata.retain(|_, data| {
            if let Some(audsrc) = data.audsrc.as_ref() {
                Self::send_buffer(audsrc, buf, frame_ms, data.start_time).is_ok()
                // If ok retain is true
            } else {
                // video only
                true
            }
        });

        Ok(())
    }

    fn send_buffer(appsrc: &AppSrc, buf: &[u8], frame_ms: u64, start_time: u64) -> AnyResult<()> {
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
        appsrc
            .push_buffer(gst_buf.copy())
            .map(|_| ())
            .map_err(|_| anyhow!("Could not push buffer to appsrc"))
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

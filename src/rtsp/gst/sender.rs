//! The component that handles passing BcMedia into
//! gstreamer media stream
use gstreamer_app::AppSrc;

use futures::stream::StreamExt;
use gstreamer::ClockTime;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use log::*;
use neolink_core::bcmedia::model::*;
use std::sync::{atomic::Ordering, Arc};
use tokio::time::{interval, Duration, Instant, MissedTickBehavior};
use tokio_stream::wrappers::ReceiverStream;

use super::{shared::*, AnyResult};

pub(super) struct NeoMediaSender {
    pub(super) data_source: ReceiverStream<BcMedia>,
    pub(super) app_source: ReceiverStream<AppSrc>,
    pub(super) shared: Arc<NeoMediaShared>,
    pub(super) appsrcs: Vec<AppSrc>,
    pub(super) waiting_for_iframe: bool,
}

impl NeoMediaSender {
    pub(super) async fn run(&mut self) -> AnyResult<()> {
        let mut resend_pause = interval(Duration::from_secs_f32(1.0f32 / 25.0f32));
        resend_pause.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut last_iframe: Option<(Instant, u64, Vec<u8>)> = None;
        loop {
            tokio::select! {
                v = self.data_source.next() => {
                    if let Some(bc_media) = v {
                        if ! self.skip_bcmedia(&bc_media)? {
                            resend_pause.reset();
                            self.inspect_bcmedia(&bc_media)?;
                            match bc_media {
                                BcMedia::Iframe(frame) => {
                                    last_iframe = Some((
                                        Instant::now(),
                                        self.shared.microseconds.load(Ordering::Relaxed),
                                        frame.data.clone(),
                                    ));
                                    self.process_buffer(&frame.data)?;
                                }
                                BcMedia::Pframe(frame) => {
                                    self.process_buffer(&frame.data)?;
                                }
                                _ => {}
                            }
                        }
                    } else {
                        break;
                    }
                }
                v = self.app_source.next() => {
                    if let Some(app_src) = v {
                        self.appsrcs.push(app_src);
                    } else {
                        break;
                    }
                },
                v = resend_pause.tick() => {
                    if let Some((time,microseconds, last_iframe)) = last_iframe.as_ref() {
                        let passed: Duration = v - *time;
                        let ms = *microseconds + passed.as_micros() as u64;
                        self.shared.microseconds.store(ms, Ordering::Relaxed);
                        self.process_buffer(last_iframe)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn process_buffer(&mut self, buf: &Vec<u8>) -> AnyResult<()> {
        let mut gst_buf = gstreamer::Buffer::with_size(buf.len()).unwrap();
        {
            let gst_buf_mut = gst_buf.get_mut().unwrap();
            let time = ClockTime::from_useconds(self.shared.microseconds.load(Ordering::Relaxed));
            gst_buf_mut.set_pts(time);
            let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
            gst_buf_data.copy_from_slice(buf);
        }

        for appsrc in self.appsrcs.iter() {
            appsrc.push_buffer(gst_buf.copy())?;
        }

        Ok(())
    }
    fn skip_bcmedia(&mut self, bc_media: &BcMedia) -> AnyResult<bool> {
        if self.waiting_for_iframe {
            if let BcMedia::Iframe(frame) = bc_media {
                let ms = frame.microseconds as u64;
                let current_ms = self.shared.microseconds.load(Ordering::Relaxed);
                if ms > current_ms {
                    self.waiting_for_iframe = false;
                } else {
                    debug!(
                        "Waiting on iframe but it was in the past: got {} but on {}",
                        ms, current_ms
                    );
                    return Ok(true);
                }
            } else {
                debug!("Waaiting on iframe");
                return Ok(true);
            }
        }
        match bc_media {
            BcMedia::Iframe(frame) => {
                let ms = frame.microseconds as u64;
                if ms < self.shared.microseconds.load(Ordering::Relaxed) {
                    debug!("Got iframe in the past");
                    return Ok(true);
                }
            }
            BcMedia::Pframe(frame) => {
                let ms = frame.microseconds as u64;
                if ms < self.shared.microseconds.load(Ordering::Relaxed) {
                    debug!("Got pframe in the past");
                    return Ok(true);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn inspect_bcmedia(&mut self, bc_media: &BcMedia) -> AnyResult<()> {
        match bc_media {
            BcMedia::Iframe(frame) => {
                match frame.video_type {
                    VideoType::H264 => {
                        self.shared
                            .vid_format
                            .store(VidFormats::H264, Ordering::Relaxed);
                    }
                    VideoType::H265 => {
                        self.shared
                            .vid_format
                            .store(VidFormats::H265, Ordering::Relaxed);
                    }
                }
                self.shared
                    .microseconds
                    .store(frame.microseconds as u64, Ordering::Relaxed);
            }
            BcMedia::Pframe(frame) => {
                match frame.video_type {
                    VideoType::H264 => {
                        self.shared
                            .vid_format
                            .store(VidFormats::H264, Ordering::Relaxed);
                    }
                    VideoType::H265 => {
                        self.shared
                            .vid_format
                            .store(VidFormats::H265, Ordering::Relaxed);
                    }
                }
                self.shared
                    .microseconds
                    .store(frame.microseconds as u64, Ordering::Relaxed);
            }
            BcMedia::Aac(_aac) => {
                self.shared
                    .aud_format
                    .store(AudFormats::Aac, Ordering::Relaxed);
            }
            BcMedia::Adpcm(_) => {
                self.shared
                    .aud_format
                    .store(AudFormats::Adpcm, Ordering::Relaxed);
            }
            _ => {}
        }
        Ok(())
    }
}

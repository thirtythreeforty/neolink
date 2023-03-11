//! The component that handles passing BcMedia into
//! gstreamer media stream
use anyhow::anyhow;
use futures::stream::StreamExt;
use gstreamer::ClockTime;
use gstreamer_app::AppSrc;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use neolink_core::bcmedia::model::*;
use std::sync::{atomic::Ordering, Arc};
use tokio::time::{interval, Duration, MissedTickBehavior};
use tokio_stream::wrappers::ReceiverStream;

use super::{shared::*, AnyResult};

#[derive(Hash, PartialEq, Eq)]
pub(super) struct ClientData {
    pub(super) appsrc: AppSrc,
    pub(super) start_time: u64,
}

pub(super) struct NeoMediaSender {
    pub(super) data_source: ReceiverStream<BcMedia>,
    pub(super) app_source: ReceiverStream<AppSrc>,
    pub(super) shared: Arc<NeoMediaShared>,
    pub(super) appsrcs: std::collections::HashSet<ClientData>,
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
        let mut resend_pause = interval(Duration::from_secs_f32(1.0f32 / 5.0f32));
        resend_pause.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut buffer: Vec<StampedData> = Default::default();
        loop {
            tokio::select! {
                v = self.data_source.next() => {
                    if let Some(bc_media) = v {
                        if ! self.skip_bcmedia(&bc_media)? {
                            resend_pause.reset();
                            self.inspect_bcmedia(&bc_media)?;
                            match bc_media {
                                BcMedia::Iframe(frame) => {
                                    buffer.clear();
                                    buffer.push(
                                        StampedData {
                                            ms: frame.microseconds as u64,
                                            data: frame.data.clone(),
                                        }
                                    );
                                    self.process_buffer(&ProcessData {
                                        live: StampedData{
                                            ms: frame.microseconds as u64,
                                            data: frame.data,
                                        },
                                        resend: buffer.as_slice()
                                    })?;
                                }
                                BcMedia::Pframe(frame) => {
                                    buffer.push(
                                        StampedData {
                                            ms: frame.microseconds as u64,
                                            data: frame.data.clone(),
                                        }
                                    );
                                    self.process_buffer(&ProcessData {
                                        live: StampedData{
                                            ms: frame.microseconds as u64,
                                            data: frame.data,
                                        },
                                        resend: buffer.as_slice()
                                    })?;
                                }
                                _ => {}
                            }
                        }
                    } else {
                        break;
                    }
                }
                v = self.app_source.next() => {
                    if let Some(appsrc) = v {
                        self.appsrcs.insert(ClientData {
                            appsrc,
                            start_time: self.shared.microseconds.load(Ordering::Relaxed)
                        });
                    } else {
                        break;
                    }
                },
                _ = resend_pause.tick() => {
                    if let Some(live) = buffer.last().cloned() {
                        self.process_buffer(&ProcessData {
                            live,
                            resend: buffer.as_slice()
                        })?;
                    }
                }
            }
        }
        Ok(())
    }

    fn process_buffer(&mut self, process_data: &ProcessData) -> AnyResult<()> {
        let frame_ms = process_data.live.ms;
        self.appsrcs.retain(|data| {
            let start_time = data.start_time;
            // If frame_ms < our start time then we just joined the stream
            // Send all data from the last iframe to catch up
            if frame_ms < start_time {
                for (idx, resend_buffer) in process_data.resend.iter().enumerate() {
                    if Self::send_buffer(&data.appsrc, resend_buffer.data.as_slice(), idx as u64, 0)
                        .is_err()
                    {
                        return false; // If fails then this appsrc is dead
                    }
                }
            }
            Self::send_buffer(
                &data.appsrc,
                process_data.live.data.as_slice(),
                process_data.live.ms,
                data.start_time,
            )
            .is_ok() // If ok retain is true
        });

        Ok(())
    }

    fn send_buffer(appsrc: &AppSrc, buf: &[u8], frame_ms: u64, start_time: u64) -> AnyResult<()> {
        let mut gst_buf = gstreamer::Buffer::with_size(buf.len()).unwrap();
        {
            let gst_buf_mut = gst_buf.get_mut().unwrap();

            let micros = if frame_ms > start_time {
                frame_ms - start_time
            } else {
                start_time
            };

            let time = ClockTime::from_useconds(micros);
            gst_buf_mut.set_pts(time);
            let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
            gst_buf_data.copy_from_slice(buf);
        }
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
                return Ok(true);
            }
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
                            .store(VidFormats::H264.into(), Ordering::Relaxed);
                    }
                    VideoType::H265 => {
                        self.shared
                            .vid_format
                            .store(VidFormats::H265.into(), Ordering::Relaxed);
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
                            .store(VidFormats::H264.into(), Ordering::Relaxed);
                    }
                    VideoType::H265 => {
                        self.shared
                            .vid_format
                            .store(VidFormats::H265.into(), Ordering::Relaxed);
                    }
                }
                self.shared
                    .microseconds
                    .store(frame.microseconds as u64, Ordering::Relaxed);
            }
            BcMedia::Aac(_aac) => {
                self.shared
                    .aud_format
                    .store(AudFormats::Aac.into(), Ordering::Relaxed);
            }
            BcMedia::Adpcm(_) => {
                self.shared
                    .aud_format
                    .store(AudFormats::Adpcm.into(), Ordering::Relaxed);
            }
            _ => {}
        }
        Ok(())
    }
}

//! Attempts to subclass GstMediaFactory
//!
//! We are now messing with gstreamer glib objects
//! expect issues

use super::{sender::*, shared::*, AnyResult};
use anyhow::{anyhow, Context};
use gstreamer::glib::object_subclass;
use gstreamer::glib::subclass::types::ObjectSubclass;
use gstreamer::{
    glib::{self, Object},
    Structure,
};
use gstreamer::{Bin, Caps, Element, ElementFactory};
use gstreamer_app::{AppSrc, AppSrcCallbacks, AppStreamType};
use gstreamer_rtsp::RTSPUrl;
use gstreamer_rtsp_server::prelude::*;
use gstreamer_rtsp_server::subclass::prelude::*;
use gstreamer_rtsp_server::RTSPMediaFactory;
use gstreamer_rtsp_server::{RTSPSuspendMode, RTSPTransportMode};
use gstreamer_rtsp_server::{RTSP_PERM_MEDIA_FACTORY_ACCESS, RTSP_PERM_MEDIA_FACTORY_CONSTRUCT};
use log::*;
use neolink_core::bcmedia::model::*;
use std::{
    collections::HashSet,
    sync::{atomic::Ordering, Arc},
};
use tokio::{
    sync::mpsc::{channel, Sender},
    task::JoinSet,
    time::Duration,
};
use tokio_stream::wrappers::ReceiverStream;

glib::wrapper! {
    /// The wrapped RTSPMediaFactory
    pub(crate) struct NeoMediaFactory(ObjectSubclass<NeoMediaFactoryImpl>) @extends RTSPMediaFactory;
}

impl Default for NeoMediaFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl NeoMediaFactory {
    pub(crate) fn new() -> Self {
        let factory = Object::new::<NeoMediaFactory>();
        factory.set_shared(false);
        // factory.set_do_retransmission(false); // Can't use as the method is missing on the 32bit docker gst dll
        factory.set_launch("videotestsrc pattern=\"snow\" ! video/x-raw,width=896,height=512,framerate=25/1 ! textoverlay name=\"inittextoverlay\" text=\"Stream not Ready\" valignment=top halignment=left font-desc=\"Sans, 32\" ! jpegenc ! rtpjpegpay name=pay0");
        factory.set_suspend_mode(RTSPSuspendMode::None);
        factory.set_transport_mode(RTSPTransportMode::PLAY);
        factory
    }

    pub(crate) fn get_sender(&self) -> Sender<BcMedia> {
        self.imp().sender.clone()
    }

    pub(crate) fn add_permitted_roles<T: AsRef<str>>(&self, permitted_roles: &HashSet<T>) {
        for permitted_role in permitted_roles {
            self.add_role_from_structure(
                &Structure::builder(permitted_role.as_ref())
                    .field(RTSP_PERM_MEDIA_FACTORY_ACCESS, true)
                    .field(RTSP_PERM_MEDIA_FACTORY_CONSTRUCT, true)
                    .build(),
            );
        }
        // During auth, first it binds anonymously. At this point it checks
        // RTSP_PERM_MEDIA_FACTORY_ACCESS to see if anyone can connect
        // This is done before the auth token is loaded, possibliy an upstream bug there
        // After checking RTSP_PERM_MEDIA_FACTORY_ACCESS anonymously
        // It loads the auth token of the user and checks that users
        // RTSP_PERM_MEDIA_FACTORY_CONSTRUCT allowing them to play
        // As a result of this we must ensure that if anonymous is not granted RTSP_PERM_MEDIA_FACTORY_ACCESS
        // As a part of permitted users then we must allow it to access
        // at least RTSP_PERM_MEDIA_FACTORY_ACCESS but not RTSP_PERM_MEDIA_FACTORY_CONSTRUCT
        // Watching Actually happens during RTSP_PERM_MEDIA_FACTORY_CONSTRUCT
        // So this should be OK to do.
        // FYI: If no RTSP_PERM_MEDIA_FACTORY_ACCESS then server returns 404 not found
        //      If yes RTSP_PERM_MEDIA_FACTORY_ACCESS but no RTSP_PERM_MEDIA_FACTORY_CONSTRUCT
        //        server returns 401 not authourised
        if !permitted_roles
            .iter()
            .map(|i| i.as_ref())
            .collect::<HashSet<&str>>()
            .contains(&"anonymous")
        {
            self.add_role_from_structure(
                &Structure::builder("anonymous")
                    .field(RTSP_PERM_MEDIA_FACTORY_ACCESS, true)
                    .build(),
            );
        }
    }

    /// This works by counting the number of acive client datas
    pub(crate) fn number_of_clients(&self) -> usize {
        self.imp().number_of_clients()
    }

    /// This returns true once an iframe + pframe set has been found
    pub(crate) fn buffer_ready(&self) -> bool {
        self.imp().buffer_ready()
    }
}

unsafe impl Send for NeoMediaFactory {}
unsafe impl Sync for NeoMediaFactory {}

pub(crate) struct NeoMediaFactoryImpl {
    sender: Sender<BcMedia>,
    clientsender: Sender<NeoMediaSender>,
    shared: Arc<NeoMediaShared>,
    #[allow(dead_code)] // Not dead just need a handle to keep it alive and drop with this obj
    threads: JoinSet<AnyResult<()>>,
}

impl Drop for NeoMediaFactoryImpl {
    fn drop(&mut self) {
        log::debug!("Dopping NeoMediaFactoryImpl");
    }
}

impl Default for NeoMediaFactoryImpl {
    fn default() -> Self {
        debug!("Constructing Factor Impl");
        let (datasender, datarx) = channel(3);
        let (clientsender, rx_clientsender) = channel(3);
        let shared: Arc<NeoMediaShared> = Default::default();

        // Prepare thread that sends data into the appsrcs
        let mut threads: JoinSet<AnyResult<()>> = Default::default();
        let mut sender = NeoMediaSenders::new(
            shared.clone(),
            ReceiverStream::new(datarx),
            ReceiverStream::new(rx_clientsender),
        );
        threads.spawn(async move {
            loop {
                tokio::task::yield_now().await;
                match sender.run().await {
                    Err(e) => {
                        warn!("Media send thead failed... restarting. Cause: {:?}", e);
                    }
                    Ok(()) => {
                        break;
                    }
                }
            }
            Ok(())
        });

        Self {
            sender: datasender,
            clientsender,
            shared,
            threads,
        }
    }
}

impl ObjectImpl for NeoMediaFactoryImpl {}
impl RTSPMediaFactoryImpl for NeoMediaFactoryImpl {
    fn create_element(&self, url: &RTSPUrl) -> Option<Element> {
        self.parent_create_element(url)
            .map(|orig| self.build_pipeline(orig).expect("Could not build pipeline"))
    }
}

#[object_subclass]
impl ObjectSubclass for NeoMediaFactoryImpl {
    const NAME: &'static str = "NeoMediaFactory";
    type Type = super::NeoMediaFactory;
    type ParentType = RTSPMediaFactory;
}

// Convenice funcion to make an element or provide a message
// about what plugin is missing
fn make_element(kind: &str, name: &str) -> AnyResult<Element> {
    ElementFactory::make_with_name(kind, Some(name)).with_context(|| {
        let plugin = match kind {
            "appsrc" => "app (gst-plugins-base)",
            "audioconvert" => "audioconvert (gst-plugins-base)",
            "adpcmdec" => "Required for audio",
            "h264parse" => "videoparsersbad (gst-plugins-bad)",
            "h265parse" => "videoparsersbad (gst-plugins-bad)",
            "rtph264pay" => "rtp (gst-plugins-good)",
            "rtph265pay" => "rtp (gst-plugins-good)",
            "aacparse" => "audioparsers (gst-plugins-good)",
            "rtpL16pay" => "rtp (gst-plugins-good)",
            "x264enc" => "x264 (gst-plugins-ugly)",
            "x265enc" => "x265 (gst-plugins-bad)",
            "avdec_h264" => "libav (gst-libav)",
            "avdec_h265" => "libav (gst-libav)",
            "videotestsrc" => "videotestsrc (gst-plugins-base)",
            "imagefreeze" => "imagefreeze (gst-plugins-good)",
            "audiotestsrc" => "audiotestsrc (gst-plugins-base)",
            "decodebin" => "playback (gst-plugins-good)",
            _ => "Unknown",
        };
        format!(
            "Missing required gstreamer plugin `{}` for `{}` element",
            plugin, kind
        )
    })
}

impl NeoMediaFactoryImpl {
    pub(crate) fn buffer_ready(&self) -> bool {
        self.shared.buffer_ready.load(Ordering::Relaxed)
    }
    pub(crate) fn number_of_clients(&self) -> usize {
        self.shared.number_of_clients.load(Ordering::Relaxed)
    }

    fn build_pipeline(&self, media: Element) -> AnyResult<Element> {
        // debug!("Building PIPELINE");
        let bin = media
            .dynamic_cast::<Bin>()
            .map_err(|_| anyhow!("Media source's element should be a bin"))?;
        // Clear the autogenerated ones
        for element in bin.iterate_elements().into_iter().flatten() {
            bin.remove(&element)?;
        }

        let mut client_data = NeoMediaSender::new();

        // Now contruct the actual ones
        match *self.shared.vid_format.blocking_read() {
            VidFormats::H265 => {
                debug!("Building H265 Pipeline");
                let source = make_element("appsrc", "vidsrc")?
                    .dynamic_cast::<AppSrc>()
                    .map_err(|_| anyhow!("Cannot cast to appsrc."))?;
                source.set_is_live(true);
                source.set_block(false);
                source.set_property("emit-signals", false);
                source.set_max_bytes(52428800);
                source.set_do_timestamp(false);
                source.set_stream_type(AppStreamType::RandomAccess);

                let need_command = client_data.get_commader();
                let enough_command = client_data.get_commader();
                let seek_command = client_data.get_commader();
                source.set_callbacks(
                    AppSrcCallbacks::builder()
                        .need_data(move |_appsrc, _amt| {
                            let _ = need_command.blocking_send(NeoMediaSenderCommand::Resume);
                        })
                        .enough_data(move |_appsrc| {
                            let _ = enough_command.blocking_send(NeoMediaSenderCommand::Pause);
                        })
                        .seek_data(move |_appsrc, seek_pos| {
                            debug!("Send seek H265");
                            let _ =
                                seek_command.blocking_send(NeoMediaSenderCommand::Seek(seek_pos));
                            true
                        })
                        .build(),
                );
                // source.set_caps(Some(
                //     &Caps::builder("video/x-h265").field("parsed", false).build(),
                // ));
                let source = source
                    .dynamic_cast::<Element>()
                    .map_err(|_| anyhow!("Cannot cast back"))?;
                let queue = make_element("queue", "source_queue")?;
                queue.set_property_from_str("leaky", "downstream");
                queue.set_property("max-size-bytes", 104857600u32);
                let parser = make_element("h265parse", "parser")?;
                // parser.set_property("config-interval", 5i32);
                let payload = make_element("rtph265pay", "pay0")?;
                bin.add_many(&[&source, &queue, &parser, &payload])?;
                Element::link_many(&[&source, &queue, &parser, &payload])?;

                let source = source
                    .dynamic_cast::<AppSrc>()
                    .map_err(|_| anyhow!("Cannot convert appsrc"))?;
                client_data.update_vid(source);
            }
            VidFormats::H264 => {
                debug!("Building H264 Pipeline");
                let source = make_element("appsrc", "vidsrc")?
                    .dynamic_cast::<AppSrc>()
                    .map_err(|_| anyhow!("Cannot cast to appsrc."))?;

                source.set_is_live(true);
                source.set_block(false);
                source.set_property("emit-signals", false);
                source.set_max_bytes(50000000u64); // 50MB
                                                   // source.set_property("max-buffers", 0u32); // v1.20
                                                   // source.set_property("max-time", Duration::from_secs_f32(2.25).as_nanos() as u64);
                source.set_do_timestamp(false);
                source.set_stream_type(AppStreamType::RandomAccess);
                let clock = source.clock();
                if let Some(clock) = clock {
                    debug!("Clock: {:?}", clock.time());
                } else {
                    debug!("No clock");
                }

                let need_command = client_data.get_commader();
                let enough_command = client_data.get_commader();
                let seek_command = client_data.get_commader();
                source.set_callbacks(
                    AppSrcCallbacks::builder()
                        .need_data(move |_appsrc, _amt| {
                            let _ = need_command.try_send(NeoMediaSenderCommand::Resume);
                        })
                        .enough_data(move |_appsrc| {
                            let _ = enough_command.try_send(NeoMediaSenderCommand::Pause);
                        })
                        .seek_data(move |_appsrc, seek_pos| {
                            debug!("Send seek H264: {}", seek_pos);
                            let result = seek_command
                                .try_send(NeoMediaSenderCommand::Seek(seek_pos))
                                .is_ok();
                            debug!("  - Sent seek H264: {}", result);
                            result
                        })
                        .build(),
                );
                // source.set_caps(Some(
                //     &Caps::builder("video/x-h264").field("parsed", false).build(),
                // ));
                let source = source
                    .dynamic_cast::<Element>()
                    .map_err(|_| anyhow!("Cannot cast back"))?;
                let queue = make_element("queue", "source_queue")?;
                queue.set_property_from_str("leaky", "downstream");
                // queue.set_property("max-size-bytes", 0u32);
                // queue.set_property("max-size-buffers", 0u32);
                queue.set_property("max-size-time", Duration::from_secs(2).as_nanos() as u64);
                let parser = make_element("h264parse", "parser")?;
                // parser.set_property("update-timecode", true);
                let payload = make_element("rtph264pay", "pay0")?;
                // payload.set_property("config-interval", 1i32);
                // let storage = make_element("rtpstorage", "vidstorage")?;
                // storage.set_property("size-time", Duration::from_secs_f32(2.25).as_nanos() as u64);
                // let jitter = make_element("rtpjitterbuffer", "pay0")?;
                // jitter.set_property("latency", Duration::from_secs_f32(2.0).as_nanos() as u32);

                // payload.set_property("config-interval", 5i32);
                bin.add_many(&[
                    &source, &queue, &parser,
                    &payload,
                    // &storage,
                    // &jitter,
                ])?;
                Element::link_many(&[
                    &source, &queue, &parser,
                    &payload,
                    // &storage,
                    // &jitter,
                ])?;

                let source = source
                    .dynamic_cast::<AppSrc>()
                    .map_err(|_| anyhow!("Cannot convert appsrc"))?;
                client_data.update_vid(source);
            }
            VidFormats::Unknown => {
                debug!("Building Unknown Pipeline");
                let source = make_element("videotestsrc", "vidsrc")?;
                source.set_property_from_str("pattern", "snow");
                let queue = make_element("queue", "queue0")?;
                queue.set_property_from_str("leaky", "downstream");
                queue.set_property("max-size-bytes", 104857600u32);
                let overlay = make_element("textoverlay", "overlay")?;
                overlay.set_property("text", "Stream not Ready");
                overlay.set_property_from_str("valignment", "top");
                overlay.set_property_from_str("halignment", "left");
                overlay.set_property("font-desc", "Sans, 16");
                let encoder = make_element("jpegenc", "encoder")?;
                let payload = make_element("rtpjpegpay", "pay0")?;

                bin.add_many(&[&source, &queue, &overlay, &encoder, &payload])?;
                source.link_filtered(
                    &queue,
                    &Caps::builder("video/x-raw")
                        .field("format", "YUY2")
                        .field("width", 896i32)
                        .field("height", 512i32)
                        .field("framerate", gstreamer::Fraction::new(25, 1))
                        .build(),
                )?;
                // source.link(&queue)?;
                Element::link_many(&[&queue, &overlay, &encoder, &payload])?;
            }
        }

        let do_aud = false;
        if do_aud {
            match *self.shared.aud_format.blocking_read() {
                AudFormats::Unknown => {}
                AudFormats::Aac => {
                    debug!("Building Aac pipeline");
                    let source = make_element("appsrc", "audsrc")?
                        .dynamic_cast::<AppSrc>()
                        .map_err(|_| anyhow!("Cannot cast to appsrc."))?;

                    source.set_is_live(true);
                    source.set_block(false);
                    source.set_property("emit-signals", false);
                    source.set_max_bytes(52428800);
                    source.set_do_timestamp(false);
                    source.set_stream_type(AppStreamType::Seekable);

                    let need_command = client_data.get_commader();
                    let enough_command = client_data.get_commader();
                    let seek_command = client_data.get_commader();
                    source.set_callbacks(
                        AppSrcCallbacks::builder()
                            .need_data(move |_appsrc, _amt| {
                                let _ = need_command.blocking_send(NeoMediaSenderCommand::Resume);
                            })
                            .enough_data(move |_appsrc| {
                                let _ = enough_command.blocking_send(NeoMediaSenderCommand::Pause);
                            })
                            .seek_data(move |_appsrc, seek_pos| {
                                debug!("Send seek AAC");
                                let _ = seek_command
                                    .blocking_send(NeoMediaSenderCommand::Seek(seek_pos));
                                true
                            })
                            .build(),
                    );

                    let source = source
                        .dynamic_cast::<Element>()
                        .map_err(|_| anyhow!("Cannot cast back"))?;

                    let queue = make_element("queue", "audqueue")?;
                    queue.set_property_from_str("leaky", "downstream");
                    queue.set_property("max-size-bytes", 104857600u32);
                    let parser = make_element("aacparse", "audparser")?;
                    let decoder = make_element("decodebin", "auddecoder")?;
                    let encoder = make_element("audioconvert", "audencoder")?;
                    let payload = make_element("rtpL16pay", "audpayload")?;
                    let storage = make_element("rtpstorage", "audstorage")?;
                    storage
                        .set_property("size-time", Duration::from_secs_f32(2.25).as_nanos() as u64);
                    let jitter = make_element("rtpjitterbuffer", "pay1")?;
                    jitter.set_property("latency", Duration::from_secs_f32(2.0).as_nanos() as u32);

                    bin.add_many(&[
                        &source, &queue, &parser, &decoder, &encoder, &payload, &storage, &jitter,
                    ])?;
                    Element::link_many(&[&source, &queue, &parser, &decoder])?;
                    Element::link_many(&[&encoder, &payload, &storage, &jitter])?;
                    decoder.connect_pad_added(move |_element, pad| {
                        debug!("Linking encoder to decoder: {:?}", pad.caps());
                        let sink_pad = encoder
                            .static_pad("sink")
                            .expect("Encoder is missing its pad");
                        pad.link(&sink_pad)
                            .expect("Failed to link AAC decoder to encoder");
                    });

                    let source = source
                        .dynamic_cast::<AppSrc>()
                        .map_err(|_| anyhow!("Cannot convert appsrc"))?;
                    client_data.update_aud(source);
                }
                AudFormats::Adpcm(block_size) => {
                    debug!("Building Adpcm pipeline");
                    // Original command line
                    // caps=audio/x-adpcm,layout=dvi,block_align={},channels=1,rate=8000
                    // ! queue silent=true max-size-bytes=10485760 min-threshold-bytes=1024
                    // ! adpcmdec
                    // ! audioconvert
                    // ! rtpL16pay name=pay1

                    let source = make_element("appsrc", "audsrc")?
                        .dynamic_cast::<AppSrc>()
                        .map_err(|_| anyhow!("Cannot cast to appsrc."))?;
                    source.set_is_live(true);
                    source.set_block(false);
                    source.set_property("emit-signals", false);
                    source.set_max_bytes(52428800);
                    source.set_do_timestamp(false);
                    source.set_stream_type(AppStreamType::RandomAccess);
                    source.set_caps(Some(
                        &Caps::builder("audio/x-adpcm")
                            .field("layout", "div")
                            .field("block_align", block_size as i32)
                            .field("channels", 1i32)
                            .field("rate", 8000i32)
                            .build(),
                    ));

                    let need_command = client_data.get_commader();
                    let enough_command = client_data.get_commader();
                    let seek_command = client_data.get_commader();
                    source.set_callbacks(
                        AppSrcCallbacks::builder()
                            .need_data(move |_appsrc, _amt| {
                                let _ = need_command.blocking_send(NeoMediaSenderCommand::Resume);
                            })
                            .enough_data(move |_appsrc| {
                                let _ = enough_command.blocking_send(NeoMediaSenderCommand::Pause);
                            })
                            .seek_data(move |_appsrc, seek_pos| {
                                debug!("Send seek Adpcm");
                                let _ = seek_command
                                    .blocking_send(NeoMediaSenderCommand::Seek(seek_pos));
                                true
                            })
                            .build(),
                    );

                    let source = source
                        .dynamic_cast::<Element>()
                        .map_err(|_| anyhow!("Cannot cast back"))?;

                    let queue = make_element("queue", "audqueue")?;
                    queue.set_property_from_str("leaky", "downstream");
                    queue.set_property("max-size-bytes", 104857600u32);
                    let decoder = make_element("decodebin", "auddecoder")?;
                    let encoder = make_element("audioconvert", "audencoder")?;
                    let payload = make_element("rtpL16pay", "pay1")?;

                    bin.add_many(&[&source, &queue, &decoder, &encoder, &payload])?;
                    Element::link_many(&[&source, &queue, &decoder])?;
                    Element::link_many(&[&encoder, &payload])?;
                    decoder.connect_pad_added(move |_element, pad| {
                        debug!("Linking encoder to decoder: {:?}", pad.caps());
                        let sink_pad = encoder
                            .static_pad("sink")
                            .expect("Encoder is missing its pad");
                        pad.link(&sink_pad)
                            .expect("Failed to link ADPCM decoder to encoder");
                    });

                    let source = source
                        .dynamic_cast::<AppSrc>()
                        .map_err(|_| anyhow!("Cannot convert appsrc"))?;
                    client_data.update_aud(source);
                }
            }
        }

        self.clientsender.blocking_send(client_data)?;
        // debug!("Pipeline built");
        bin.dynamic_cast::<Element>()
            .map_err(|_| anyhow!("Cannot cast back"))
    }
}

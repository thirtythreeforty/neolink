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
use gstreamer::{Bin, Caps, ClockTime, Element, ElementFactory};
use gstreamer_app::{AppSrc, AppStreamType};
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
        let factory = Object::new::<NeoMediaFactory>(&[]);
        factory.set_shared(false);
        factory.set_do_retransmission(false);
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
            self.add_role_from_structure(&Structure::new(
                permitted_role.as_ref(),
                &[
                    (*RTSP_PERM_MEDIA_FACTORY_ACCESS, &true),
                    (*RTSP_PERM_MEDIA_FACTORY_CONSTRUCT, &true),
                ],
            ));
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
            self.add_role_from_structure(&Structure::new(
                "anonymous",
                &[(*RTSP_PERM_MEDIA_FACTORY_ACCESS, &true)],
            ));
        }
    }
}

unsafe impl Send for NeoMediaFactory {}
unsafe impl Sync for NeoMediaFactory {}

pub(crate) struct NeoMediaFactoryImpl {
    sender: Sender<BcMedia>,
    appsender: Sender<AppSrc>,
    shared: Arc<NeoMediaShared>,
    #[allow(dead_code)] // Not dead just need a handle to keep it alive and drop with this obj
    threads: JoinSet<AnyResult<()>>,
}

impl Drop for NeoMediaFactoryImpl {
    fn drop(&mut self) {
        log::info!("Dopping NeoMediaFactoryImpl");
    }
}

impl Default for NeoMediaFactoryImpl {
    fn default() -> Self {
        warn!("Constructing Factor Impl");
        let (datasender, datarx) = channel(3);
        let (appsender, rx_appsender) = channel(3);
        let shared: Arc<NeoMediaShared> = Default::default();

        // Prepare thread that sends data into the appsrcs
        let mut threads: JoinSet<AnyResult<()>> = Default::default();
        let mut sender = NeoMediaSender {
            data_source: ReceiverStream::new(datarx),
            app_source: ReceiverStream::new(rx_appsender),
            shared: shared.clone(),
            appsrcs: Default::default(),
            waiting_for_iframe: true,
        };
        threads.spawn(async move { sender.run().await });

        Self {
            sender: datasender,
            appsender,
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
    fn build_pipeline(&self, media: Element) -> AnyResult<Element> {
        let bin = media
            .dynamic_cast::<Bin>()
            .map_err(|_| anyhow!("Media source's element should be a bin"))?;
        // Clear the autogenerated ones
        for element in bin.iterate_elements().into_iter().flatten() {
            bin.remove(&element)?;
        }

        debug!("self.shared owners: {}", Arc::strong_count(&self.shared));
        // Now contruct the actual ones
        match VidFormats::from(self.shared.vid_format.load(Ordering::Relaxed)) {
            VidFormats::H265 => {
                debug!("Building H265 Pipeline");
                let source = make_element("appsrc", "vidsrc")?
                    .dynamic_cast::<AppSrc>()
                    .map_err(|_| anyhow!("Cannot cast to appsrc."))?;
                source.set_base_time(ClockTime::from_mseconds(
                    self.shared.microseconds.load(Ordering::Relaxed),
                ));
                source.set_is_live(true);
                source.set_block(true);
                source.set_property("emit-signals", &false);
                source.set_max_bytes(52428800);
                source.set_do_timestamp(false);
                source.set_stream_type(AppStreamType::Stream);
                // source.set_caps(Some(
                //     &Caps::builder("video/x-h265").field("parsed", false).build(),
                // ));
                let source = source
                    .dynamic_cast::<Element>()
                    .map_err(|_| anyhow!("Cannot cast back"))?;
                let queue = make_element("queue", "source_queue")?;
                let parser = make_element("h265parse", "parser")?;
                let payload = make_element("rtph265pay", "pay0")?;
                bin.add_many(&[&source, &queue, &parser, &payload])?;
                Element::link_many(&[&source, &queue, &parser, &payload])?;

                let source = source
                    .dynamic_cast::<AppSrc>()
                    .map_err(|_| anyhow!("Cannot convert appsrc"))?;
                self.appsender.blocking_send(source)?;
            }
            VidFormats::H264 => {
                debug!("Building H264 Pipeline");
                let source = make_element("appsrc", "vidsrc")?
                    .dynamic_cast::<AppSrc>()
                    .map_err(|_| anyhow!("Cannot cast to appsrc."))?;
                source.set_base_time(ClockTime::from_mseconds(
                    self.shared.microseconds.load(Ordering::Relaxed),
                ));
                source.set_is_live(true);
                source.set_block(true);
                source.set_property("emit-signals", &false);
                source.set_max_bytes(52428800);
                source.set_do_timestamp(false);
                source.set_stream_type(AppStreamType::Stream);
                // source.set_caps(Some(
                //     &Caps::builder("video/x-h264").field("parsed", false).build(),
                // ));
                let source = source
                    .dynamic_cast::<Element>()
                    .map_err(|_| anyhow!("Cannot cast back"))?;
                let queue = make_element("queue", "source_queue")?;
                let parser = make_element("h264parse", "parser")?;
                let payload = make_element("rtph264pay", "pay0")?;
                bin.add_many(&[&source, &queue, &parser, &payload])?;
                Element::link_many(&[&source, &queue, &parser, &payload])?;

                let source = source
                    .dynamic_cast::<AppSrc>()
                    .map_err(|_| anyhow!("Cannot convert appsrc"))?;
                self.appsender.blocking_send(source)?;
            }
            VidFormats::Unknown => {
                debug!("Building Unknown Pipeline");
                let source = make_element("videotestsrc", "vidsrc")?;
                source.set_property_from_str("pattern", "snow");
                let queue = make_element("queue", "queue0")?;
                let overlay = make_element("textoverlay", "overlay")?;
                overlay.set_property("text", "Stream not Ready");
                overlay.set_property_from_str("valignment", "top");
                overlay.set_property_from_str("halignment", "left");
                overlay.set_property("font-desc", "Sans, 32");
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
                queue.link(&overlay)?;
                overlay.link(&encoder)?;
                encoder.link(&payload)?;
            }
        }

        bin.dynamic_cast::<Element>()
            .map_err(|_| anyhow!("Cannot cast back"))
    }
}

//! Attempts to subclass GstMediaFactory
//!
//! We are now messing with gstreamer glib objects
//! expect issues

use super::AnyResult;
use crate::common::{AudFormat, NeoReactor, StreamInstance, VidFormat};
use anyhow::{anyhow, Context};
use async_channel::{Receiver as AsyncReceiver, Sender as AsyncSender};
use futures::stream::StreamExt;
use gstreamer::glib::object_subclass;
use gstreamer::glib::subclass::types::ObjectSubclass;
use gstreamer::{
    glib::{self, Object},
    prelude::*,
    ClockTime, Structure,
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
use neolink_core::{bc_protocol::StreamKind, bcmedia::model::*};
use std::collections::HashSet;
use std::convert::TryInto;
use std::sync::Arc;
use tokio::{
    sync::{
        mpsc::{channel as mpsc, Sender as MpscSender},
        oneshot::{channel as oneshot, Receiver as OneshotReceiver, Sender as OneshotSender},
        Mutex, RwLock,
    },
    task::JoinSet,
    time::Duration,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

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
    fn new() -> Self {
        let factory = Object::new::<NeoMediaFactory>();
        factory.set_shared(false);
        factory.set_launch("videotestsrc pattern=\"snow\" ! video/x-raw,width=896,height=512,framerate=25/1 ! textoverlay name=\"inittextoverlay\" text=\"Stream not Ready\" valignment=top halignment=left font-desc=\"Sans, 32\" ! jpegenc ! rtpjpegpay name=pay0");
        factory.set_suspend_mode(RTSPSuspendMode::None);
        factory.set_transport_mode(RTSPTransportMode::PLAY);
        factory
    }

    pub(crate) async fn new_with_callback<F>(callback: F) -> AnyResult<Self>
    where
        F: Fn(Element) -> AnyResult<Option<Element>> + Send + Sync + 'static,
    {
        let factory = Self::new();
        factory.imp().set_callback(callback).await;
        Ok(factory)
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
}

unsafe impl Send for NeoMediaFactory {}
unsafe impl Sync for NeoMediaFactory {}

pub(crate) struct NeoMediaFactoryImpl {
    call_back: Arc<Mutex<Option<Arc<dyn Fn(Element) -> AnyResult<Option<Element>> + Send + Sync>>>>,
}

impl Default for NeoMediaFactoryImpl {
    fn default() -> Self {
        debug!("Constructing Factor Impl");
        // Prepare thread that sends data into the appsrcs
        Self {
            call_back: Arc::new(Mutex::new(None)),
        }
    }
}

impl NeoMediaFactoryImpl {
    async fn set_callback<F>(&self, callback: F)
    where
        F: Fn(Element) -> AnyResult<Option<Element>> + Send + Sync + 'static,
    {
        self.call_back.lock().await.replace(Arc::new(callback));
    }
    fn build_pipeline(&self, media: Element) -> AnyResult<Option<Element>> {
        match self.call_back.blocking_lock().as_ref() {
            Some(call) => call(media),
            None => Ok(None),
        }
    }
}

impl ObjectImpl for NeoMediaFactoryImpl {}
impl RTSPMediaFactoryImpl for NeoMediaFactoryImpl {
    fn create_element(&self, url: &RTSPUrl) -> Option<Element> {
        self.parent_create_element(url)
            .and_then(|orig| self.build_pipeline(orig).expect("Could not build pipeline"))
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

fn make_queue(name: &str) -> AnyResult<Element> {
    let queue = make_element("queue", name)?;
    queue.set_property_from_str("leaky", "downstream");
    queue.set_property("max-size-bytes", 0u32);
    queue.set_property("max-size-buffers", 0u32);
    queue.set_property(
        "max-size-time",
        std::convert::TryInto::<u64>::try_into(tokio::time::Duration::from_secs(5).as_nanos())
            .unwrap_or(0),
    );
    Ok(queue)
}

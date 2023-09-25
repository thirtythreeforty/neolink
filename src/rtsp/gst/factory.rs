//! Attempts to subclass GstMediaFactory
//!
//! We are now messing with gstreamer glib objects
//! expect issues

use super::AnyResult;
use gstreamer::glib::object_subclass;
use gstreamer::glib::subclass::types::ObjectSubclass;
use gstreamer::Element;
use gstreamer::{
    glib::{self, Object},
    Structure,
};
use gstreamer_rtsp::RTSPUrl;
use gstreamer_rtsp_server::prelude::*;
use gstreamer_rtsp_server::subclass::prelude::*;
use gstreamer_rtsp_server::RTSPMediaFactory;
use gstreamer_rtsp_server::{RTSPSuspendMode, RTSPTransportMode};
use gstreamer_rtsp_server::{RTSP_PERM_MEDIA_FACTORY_ACCESS, RTSP_PERM_MEDIA_FACTORY_CONSTRUCT};
use log::*;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

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
            let s = permitted_role.as_ref();
            log::debug!("Adding {} as permitted user", s);
            self.add_role_from_structure(
                &Structure::builder(s)
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
    #[allow(clippy::type_complexity)]
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

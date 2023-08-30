//! Attempts to subclass RtspServer
//!
//! We are now messing with gstreamer glib objects
//! expect issues

use super::{factory::*, AnyResult};
use crate::config::*;

use anyhow::{anyhow, Context};
use gstreamer::glib::{self, object_subclass, subclass::types::ObjectSubclass, MainLoop, Object};
use gstreamer_rtsp::RTSPAuthMethod;
use gstreamer_rtsp_server::{
    gio::{TlsAuthenticationMode, TlsCertificate},
    prelude::*,
    subclass::prelude::*,
    RTSPAuth, RTSPFilterResult, RTSPServer, RTSPToken, RTSP_TOKEN_MEDIA_FACTORY_ROLE,
};
use log::*;
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fs,
    sync::Arc,
};
use tokio::{
    sync::{mpsc::Sender, RwLock},
    task::JoinSet,
    time::{timeout, Duration},
};

glib::wrapper! {
    /// The wrapped RTSPServer
    pub(crate) struct NeoRtspServer(ObjectSubclass<NeoRtspServerImpl>) @extends RTSPServer;
}

impl Default for NeoRtspServer {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

impl NeoRtspServer {
    pub(crate) fn new() -> AnyResult<Self> {
        gstreamer::init().context("Gstreamer failed to initialise")?;
        let factory = Object::new::<NeoRtspServer>();
        Ok(factory)
    }

    pub(crate) async fn add_permitted_roles<T: Into<String>, U: AsRef<str>>(
        &self,
        tag: T,
        permitted_users: &HashSet<U>,
    ) -> AnyResult<()> {
        self.imp().add_permitted_roles(tag, permitted_users).await
    }

    pub(crate) async fn run(&self, bind_addr: &str, bind_port: u16) -> AnyResult<()> {
        let server = self;
        server.set_address(bind_addr);
        server.set_service(&format!("{}", bind_port));
        // Attach server to default Glib context
        let _ = server.attach(None);
        let main_loop = Arc::new(MainLoop::new(None, false));
        // Run the Glib main loop.
        let main_loop_thread = main_loop.clone();
        let handle = tokio::task::spawn_blocking(move || {
            main_loop_thread.run();
            AnyResult::Ok(())
        });
        timeout(Duration::from_secs(5), self.imp().threads.write())
            .await
            .with_context(|| "Timeout waiting to lock Server threads")?
            .spawn(async move { handle.await? });
        timeout(Duration::from_secs(5), self.imp().main_loop.write())
            .await
            .with_context(|| "Timeout waiting to lock Server main_loop")?
            .replace(main_loop);
        Ok(())
    }

    pub(crate) async fn quit(&self) -> AnyResult<()> {
        if let Some(main_loop) = self.imp().main_loop.read().await.as_ref() {
            main_loop.quit();
        }
        Ok(())
    }

    pub(crate) async fn join(&self) -> AnyResult<()> {
        let mut threads = self.imp().threads.write().await;
        while let Some(thread) = threads.join_next().await {
            thread??;
        }
        Ok(())
    }

    pub(crate) fn set_up_tls(&self, config: &Config) {
        self.imp().set_up_tls(config)
    }

    pub(crate) fn set_up_users(&self, users: &[UserConfig]) {
        self.imp().set_up_users(users)
    }
}

unsafe impl Send for NeoRtspServer {}
unsafe impl Sync for NeoRtspServer {}

struct FactoryData {
    factory: NeoMediaFactory,
    paths: HashSet<String>,
}

#[derive(Default)]
pub(crate) struct NeoRtspServerImpl {
    medias: RwLock<HashMap<String, FactoryData>>,
    threads: RwLock<JoinSet<AnyResult<()>>>,
    main_loop: RwLock<Option<Arc<MainLoop>>>,
}

impl ObjectImpl for NeoRtspServerImpl {}
impl RTSPServerImpl for NeoRtspServerImpl {}

#[object_subclass]
impl ObjectSubclass for NeoRtspServerImpl {
    const NAME: &'static str = "NeoRtspServer";
    type Type = NeoRtspServer;
    type ParentType = RTSPServer;
}

impl NeoRtspServerImpl {
    pub(crate) async fn add_permitted_roles<T: Into<String>, U: AsRef<str>>(
        &self,
        tag: T,
        permitted_users: &HashSet<U>,
    ) -> AnyResult<()> {
        let tag: String = tag.into();
        if let Some(media) = self.medias.write().await.get_mut(&tag) {
            media.factory.add_permitted_roles(permitted_users);
            Ok(())
        } else {
            Err(anyhow!("No media with tag {} to add users to", &tag))
        }
    }

    pub(crate) fn set_credentials(&self, credentials: &[(&str, &str)]) -> AnyResult<()> {
        let auth = self.obj().auth().unwrap_or_else(RTSPAuth::new);
        auth.set_supported_methods(RTSPAuthMethod::Basic);

        let mut un_authtoken = RTSPToken::new(&[(RTSP_TOKEN_MEDIA_FACTORY_ROLE, &"anonymous")]);
        auth.set_default_token(Some(&mut un_authtoken));

        for credential in credentials {
            let (user, pass) = credential;
            trace!("Setting credentials for user {}", user);
            let token = RTSPToken::new(&[(RTSP_TOKEN_MEDIA_FACTORY_ROLE, user)]);
            let basic = RTSPAuth::make_basic(user, pass);
            auth.add_basic(basic.as_str(), &token);
        }

        self.obj().set_auth(Some(&auth));
        Ok(())
    }

    pub(crate) fn set_tls(
        &self,
        cert_file: &str,
        client_auth: TlsAuthenticationMode,
    ) -> AnyResult<()> {
        debug!("Setting up TLS using {}", cert_file);
        let auth = self.obj().auth().unwrap_or_else(RTSPAuth::new);

        // We seperate reading the file and changing to a PEM so that we get different error messages.
        let cert_contents = fs::read_to_string(cert_file).with_context(|| "TLS file not found")?;
        let cert = TlsCertificate::from_pem(&cert_contents)
            .with_context(|| "Not a valid TLS certificate")?;
        auth.set_tls_certificate(Some(&cert));
        auth.set_tls_authentication_mode(client_auth);

        self.obj().set_auth(Some(&auth));
        Ok(())
    }

    pub(crate) fn set_up_tls(&self, config: &Config) {
        let tls_client_auth = match &config.tls_client_auth as &str {
            "request" => TlsAuthenticationMode::Requested,
            "require" => TlsAuthenticationMode::Required,
            "none" => TlsAuthenticationMode::None,
            _ => unreachable!(),
        };
        if let Some(cert_path) = &config.certificate {
            self.set_tls(cert_path, tls_client_auth)
                .expect("Failed to set up TLS");
        }
    }

    pub(crate) fn set_up_users(&self, users: &[UserConfig]) {
        // Setting up users
        let credentials: Vec<_> = users
            .iter()
            .map(|user| (&*user.name, &*user.pass))
            .collect();
        self.set_credentials(&credentials)
            .expect("Failed to set up users");
    }
}

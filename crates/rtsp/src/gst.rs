//! This module provides an "RtspServer" abstraction that allows consumers of its API to feed it
//! data using an ordinary std::io::Write interface.
pub use self::maybe_app_src::MaybeAppSrc;
use gstreamer::prelude::Cast;
use gstreamer::{Bin, Structure};
use gstreamer_app::AppSrc;
//use gstreamer_rtsp::RTSPLowerTrans;
use gio::{TlsAuthenticationMode, TlsCertificate};
use gstreamer_rtsp::RTSPAuthMethod;
use gstreamer_rtsp_server::prelude::*;
use gstreamer_rtsp_server::{
    RTSPAuth, RTSPMediaFactory, RTSPServer as GstRTSPServer, RTSPToken,
    RTSP_PERM_MEDIA_FACTORY_ACCESS, RTSP_PERM_MEDIA_FACTORY_CONSTRUCT,
    RTSP_TOKEN_MEDIA_FACTORY_ROLE,
};
use log::*;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::Write;

type Result<T> = std::result::Result<T, ()>;

pub struct RtspServer {
    server: GstRTSPServer,
}

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub enum StreamFormat {
    H264,
    H265,
    AAC,
    ADPCM,
}

pub struct GstOutputs {
    pub audsrc: MaybeAppSrc,
    pub vidsrc: MaybeAppSrc,
    video_format: Option<StreamFormat>,
    audio_format: Option<StreamFormat>,
    factory: RTSPMediaFactory,
}

impl GstOutputs {
    pub fn from_appsrcs(vidsrc: MaybeAppSrc, audsrc: MaybeAppSrc) -> GstOutputs {
        let result = GstOutputs {
            vidsrc,
            audsrc,
            video_format: None,
            audio_format: None,
            factory: RTSPMediaFactory::new(),
        };
        result.apply_format();
        result
    }

    pub fn set_format(&mut self, format: Option<StreamFormat>) {
        match format {
            Some(StreamFormat::H264) | Some(StreamFormat::H265) => {
                if format != self.video_format {
                    self.video_format = format;
                    self.apply_format();
                }
            }
            Some(StreamFormat::AAC) | Some(StreamFormat::ADPCM) => {
                if format != self.audio_format {
                    self.audio_format = format;
                    self.apply_format();
                }
            }
            _ => {}
        }
    }

    fn apply_format(&self) {
        let launch_vid = match self.video_format {
            Some(StreamFormat::H264) => {
                "! queue silent=true max-size-bytes=10485760  min-threshold-bytes=1024 ! h264parse ! rtph264pay name=pay0"
            }
            Some(StreamFormat::H265) => {
                "! queue silent=true  max-size-bytes=10485760  min-threshold-bytes=1024 ! h265parse ! rtph265pay name=pay0"
            }
            _ => "! fakesink",
        };

        let launch_aud = match self.audio_format {
            Some(StreamFormat::ADPCM) => "! queue silent=true max-size-bytes=10485760 min-threshold-bytes=1024 ! rawaudioparse format=pcm pcm-format=s16le sample-rate=8000 num-channels=1 interleaved=true ! audioconvert ! rtpL16pay name=pay1", // DVI4 is converted to pcm in the appsrc
            Some(StreamFormat::AAC) => "! queue silent=true max-size-bytes=10485760 min-threshold-bytes=1024 ! aacparse ! decodebin ! audioconvert ! rtpL16pay name=pay1 name=pay1",
            _ => "! fakesink",
        };

        self.factory.set_launch(
            &vec![
            "( ",
            "appsrc name=vidsrc is-live=true block=true emit-signals=false max-bytes=52428800 do-timestamp=true format=GST_FORMAT_TIME", // 50MB max size so that it won't grow to infinite if the queue blocks
            launch_vid,
            "appsrc name=audsrc is-live=true block=true emit-signals=false max-bytes=52428800 do-timestamp=true format=GST_FORMAT_TIME", // 50MB max size so that it won't grow to infinite if the queue blocks
            launch_aud,
            ")"
        ]
            .join(" "),
        );
    }
}

impl Default for RtspServer {
    fn default() -> RtspServer {
        Self::new()
    }
}

impl RtspServer {
    pub fn new() -> RtspServer {
        gstreamer::init().expect("Gstreamer should not explode");
        RtspServer {
            server: GstRTSPServer::new(),
        }
    }

    pub fn add_stream(
        &self,
        paths: &[&str],
        permitted_users: &HashSet<&str>,
    ) -> Result<GstOutputs> {
        let mounts = self
            .server
            .get_mount_points()
            .expect("The server should have mountpoints");

        // Create a MaybeAppSrc: Write which we will give the caller.  When the backing AppSrc is
        // created by the factory, fish it out and give it to the waiting MaybeAppSrc via the
        // channel it provided.  This callback may be called more than once by Gstreamer if it is
        // unhappy with the pipeline, so keep updating the MaybeAppSrc.
        let (maybe_app_src, tx) = MaybeAppSrc::new_with_tx();
        let (maybe_app_src_aud, tx_aud) = MaybeAppSrc::new_with_tx();

        let outputs = GstOutputs::from_appsrcs(maybe_app_src, maybe_app_src_aud);

        let factory = &outputs.factory;

        debug!(
            "Permitting {} to access {}",
            // This is hashmap or (iter) equivalent of join, it requres itertools
            itertools::Itertools::intersperse(permitted_users.iter().cloned(), ", ")
                .collect::<String>(),
            paths.join(", ")
        );
        self.add_permitted_roles(factory, permitted_users);

        factory.set_shared(true);

        factory.connect_media_configure(move |_factory, media| {
            debug!("RTSP: media was configured");
            let bin = media
                .get_element()
                .expect("Media should have an element")
                .dynamic_cast::<Bin>()
                .expect("Media source's element should be a bin");
            let app_src = bin
                .get_by_name_recurse_up("vidsrc")
                .expect("write_src must be present in created bin")
                .dynamic_cast::<AppSrc>()
                .expect("Source element is expected to be an appsrc!");
            let _ = tx.send(app_src); // Receiver may be dropped, don't panic if so

            let app_src_aud = bin
                .get_by_name_recurse_up("audsrc")
                .expect("write_src must be present in created bin")
                .dynamic_cast::<AppSrc>()
                .expect("Source element is expected to be an appsrc!");
            let _ = tx_aud.send(app_src_aud); // Receiver may be dropped, don't panic if so
        });

        for path in paths {
            mounts.add_factory(path, factory);
        }

        Ok(outputs)
    }

    pub fn add_permitted_roles(&self, factory: &RTSPMediaFactory, permitted_roles: &HashSet<&str>) {
        for permitted_role in permitted_roles {
            factory.add_role_from_structure(&Structure::new(
                permitted_role,
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
        if !permitted_roles.contains(&"anonymous") {
            factory.add_role_from_structure(&Structure::new(
                "anonymous",
                &[(*RTSP_PERM_MEDIA_FACTORY_ACCESS, &true)],
            ));
        }
    }

    pub fn set_credentials(&self, credentials: &[(&str, &str)]) -> Result<()> {
        let auth = self.server.get_auth().unwrap_or_else(RTSPAuth::new);
        auth.set_supported_methods(RTSPAuthMethod::Basic);

        let mut un_authtoken = RTSPToken::new(&[(*RTSP_TOKEN_MEDIA_FACTORY_ROLE, &"anonymous")]);
        auth.set_default_token(Some(&mut un_authtoken));

        for credential in credentials {
            let (user, pass) = credential;
            trace!("Setting credentials for user {}", user);
            let token = RTSPToken::new(&[(*RTSP_TOKEN_MEDIA_FACTORY_ROLE, user)]);
            let basic = RTSPAuth::make_basic(user, pass);
            auth.add_basic(basic.as_str(), &token);
        }

        self.server.set_auth(Some(&auth));
        Ok(())
    }

    pub fn set_tls(&self, cert_file: &str, client_auth: TlsAuthenticationMode) -> Result<()> {
        debug!("Setting up TLS using {}", cert_file);
        let auth = self.server.get_auth().unwrap_or_else(RTSPAuth::new);

        // We seperate reading the file and changing to a PEM so that we get different error messages.
        let cert_contents = fs::read_to_string(cert_file).expect("TLS file not found");
        let cert = TlsCertificate::from_pem(&cert_contents).expect("Not a valid TLS certificate");
        auth.set_tls_certificate(Some(&cert));
        auth.set_tls_authentication_mode(client_auth);

        self.server.set_auth(Some(&auth));
        Ok(())
    }

    pub fn run(&self, bind_addr: &str, bind_port: u16) {
        self.server.set_address(bind_addr);
        self.server.set_service(&format!("{}", bind_port));
        // Attach server to default Glib context
        self.server.attach(None);

        // Run the Glib main loop.
        let main_loop = glib::MainLoop::new(None, false);
        main_loop.run();
    }
}

mod maybe_app_src {
    use super::*;
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

    /// A Write implementation around AppSrc that also allows delaying the creation of the AppSrc
    /// until later, discarding written data until the AppSrc is provided.
    pub struct MaybeAppSrc {
        rx: Receiver<AppSrc>,
        app_src: Option<AppSrc>,
    }

    impl MaybeAppSrc {
        /// Creates a MaybeAppSrc.  Also returns a Sender that you must use to provide an AppSrc as
        /// soon as one is available.  When it is received, the MaybeAppSrc will start pushing data
        /// into the AppSrc when write() is called.
        pub fn new_with_tx() -> (Self, SyncSender<AppSrc>) {
            let (tx, rx) = sync_channel(3); // The sender should not send very often
            (MaybeAppSrc { rx, app_src: None }, tx)
        }

        /// Flushes data to Gstreamer on a problem communicating with the underlying video source.
        pub fn on_stream_error(&mut self) {
            if let Some(src) = self.try_get_src() {
                // Ignore "errors" from Gstreamer such as FLUSHING, which are not really errors.
                let _ = src.end_of_stream();
            }
        }

        /// Attempts to retrieve the AppSrc that should be passed in by the caller of new_with_tx
        /// at some point after this struct has been created.  At that point, we swap over to
        /// owning the AppSrc directly.  This function handles either case and returns the AppSrc,
        /// or None if the caller has not yet sent one.
        fn try_get_src(&mut self) -> Option<&AppSrc> {
            while let Some(src) = self.rx.try_recv().ok() {
                self.app_src = Some(src);
            }
            self.app_src.as_ref()
        }
    }

    impl Write for MaybeAppSrc {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            // If we have no AppSrc yet, throw away the data and claim that it was written
            let app_src = match self.try_get_src() {
                Some(src) => src,
                None => return Ok(buf.len()),
            };

            let mut gst_buf = gstreamer::Buffer::with_size(buf.len()).unwrap();
            {
                let gst_buf_mut = gst_buf.get_mut().unwrap();
                let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
                gst_buf_data.copy_from_slice(buf);
            }

            let res = app_src.push_buffer(gst_buf); //.map_err(|e| io::Error::new(io::ErrorKind::Other, Box::new(e)))?;
            if res.is_err() {
                self.app_src = None;
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

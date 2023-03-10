//! This module provides an "RtspServer" abstraction that allows consumers of its API to feed it
//! data using an ordinary std::io::Write interface.
mod maybe_app_src;
mod maybe_inputselect;

pub(crate) use self::maybe_app_src::MaybeAppSrc;
pub(crate) use self::maybe_inputselect::MaybeInputSelect;

// use super::adpcm::adpcm_to_pcm;
// use super::errors::Error;
use gstreamer::prelude::Cast;
use gstreamer::{Bin, ElementFactory, Structure};
use gstreamer_app::AppSrc;
//use gstreamer_rtsp::RTSPLowerTrans;
use anyhow::{anyhow, Context};
use gstreamer_rtsp::RTSPAuthMethod;
pub use gstreamer_rtsp_server::gio::{TlsAuthenticationMode, TlsCertificate};
use gstreamer_rtsp_server::glib;
use gstreamer_rtsp_server::prelude::*;
use gstreamer_rtsp_server::{
    RTSPAuth, RTSPMediaFactory, RTSPServer as GstRTSPServer, RTSPToken,
    RTSP_PERM_MEDIA_FACTORY_ACCESS, RTSP_PERM_MEDIA_FACTORY_CONSTRUCT,
    RTSP_TOKEN_MEDIA_FACTORY_ROLE,
};
use log::*;
use neolink_core::bcmedia::model::*;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::Write;
use std::sync::Arc;

type Result<T> = std::result::Result<T, ()>;
type AnyResult<T> = std::result::Result<T, anyhow::Error>;

pub(crate) struct RtspServer {
    server: GstRTSPServer,
    main_loop: Arc<glib::MainLoop>,
}

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub(crate) enum InputMode {
    Live,
    Paused,
}

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub(crate) enum PausedSources {
    TestSrc,
    Still,
    Black,
    None,
}

pub(crate) struct GstOutputs {
    pub(crate) audsrc: MaybeAppSrc,
    pub(crate) vidsrc: MaybeAppSrc,
    vid_inputselect: MaybeInputSelect,
    aud_inputselect: MaybeInputSelect,
    video_format: Option<StreamFormat>,
    audio_format: Option<StreamFormat>,
    factory: RTSPMediaFactory,
    when_paused: PausedSources,
    last_iframe: Option<Vec<u8>>,
}

// The stream from the camera will be using one of these formats
//
// This is used as part of `StreamOutput` to give hints about
// the format of the stream
#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
enum StreamFormat {
    // H264 (AVC) video format
    H264,
    // H265 (HEVC) video format
    H265,
    // AAC audio
    Aac,
    // ADPCM in DVI-4 format
    Adpcm(u16),
}

impl GstOutputs {
    pub(crate) fn from_appsrcs(
        vidsrc: MaybeAppSrc,
        audsrc: MaybeAppSrc,
        vid_inputselect: MaybeInputSelect,
        aud_inputselect: MaybeInputSelect,
    ) -> GstOutputs {
        let result = GstOutputs {
            vidsrc,
            audsrc,
            vid_inputselect,
            aud_inputselect,
            video_format: None,
            audio_format: None,
            when_paused: PausedSources::None,
            factory: RTSPMediaFactory::new(),
            last_iframe: Default::default(),
        };
        result.apply_format();
        result
    }

    pub(crate) fn stream_recv(&mut self, media: BcMedia) -> AnyResult<()> {
        // Ensure stream is on cam mode
        match media {
            BcMedia::Iframe(payload) => {
                let video_type = match payload.video_type {
                    VideoType::H264 => StreamFormat::H264,
                    VideoType::H265 => StreamFormat::H265,
                };
                self.set_format(Some(video_type));
                self.vidsrc
                    .write_all(&payload.data)
                    .with_context(|| "Cannot write IFrame to vidsrc")?;
                self.last_iframe = Some(payload.data);
            }
            BcMedia::Pframe(payload) => {
                let video_type = match payload.video_type {
                    VideoType::H264 => StreamFormat::H264,
                    VideoType::H265 => StreamFormat::H265,
                };
                self.set_format(Some(video_type));
                self.vidsrc
                    .write_all(&payload.data)
                    .with_context(|| "Cannot write PFrame to vidsrc")?;
            }
            BcMedia::Aac(payload) => {
                self.set_format(Some(StreamFormat::Aac));
                self.audsrc
                    .write_all(&payload.data)
                    .with_context(|| "Cannot write AAC to audsrc")?;
            }
            BcMedia::Adpcm(payload) => {
                self.set_format(Some(StreamFormat::Adpcm(payload.data.len() as u16)));
                self.audsrc
                    .write_all(&payload.data)
                    .with_context(|| "Cannot write ADPCM to audsrc")?;
            }
            _ => {
                //Ignore other BcMedia like InfoV1 and InfoV2
            }
        }

        Ok(())
    }

    pub(crate) fn set_input_source(&mut self, input_source: InputMode) -> AnyResult<()> {
        // PausedSources::None is exceptional in that it dosen't swap the stream
        // to an alterntive source. It just repeats the last buffer meaning not re-encoding
        // at the cost some clients not handeling the repeating buffers well
        if self.when_paused != PausedSources::None {
            match &input_source {
                InputMode::Live => {
                    self.vid_inputselect.set_input(0)?;
                    self.aud_inputselect.set_input(0)?;
                }
                InputMode::Paused => {
                    self.vid_inputselect.set_input(1)?;
                    self.aud_inputselect.set_input(1)?;
                }
            }
        } else {
            // For PausedSources::None we still want to use the silence as we don't want to
            // repeat the audio buffers
            match &input_source {
                InputMode::Live => {
                    self.aud_inputselect.set_input(0)?;
                }
                InputMode::Paused => {
                    self.aud_inputselect.set_input(1)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn set_paused_source(&mut self, paused_source: PausedSources) {
        self.when_paused = paused_source;
        self.apply_format();
    }

    pub(crate) fn has_last_iframe(&self) -> bool {
        self.last_iframe.is_some()
    }

    pub(crate) fn write_last_iframe(&mut self) -> AnyResult<()> {
        self.vidsrc.write_all(
            self.last_iframe
                .as_ref()
                .ok_or_else(|| anyhow!("No iframe data avaliable"))?
                .as_slice(),
        )?;
        Ok(())
    }

    fn set_format(&mut self, format: Option<StreamFormat>) {
        match format {
            Some(StreamFormat::H264) | Some(StreamFormat::H265) => {
                if format != self.video_format {
                    self.video_format = format;
                    self.apply_format();
                }
            }
            Some(StreamFormat::Aac) | Some(StreamFormat::Adpcm(_)) => {
                if format != self.audio_format {
                    self.audio_format = format;
                    self.apply_format();
                }
            }
            _ => {}
        }
    }

    fn apply_format(&self) {
        // This is the final sink prior to rtsp
        // - In the case of PausedSources::None we just pass through without encoding
        // - In the case of all other pause sources we need to recencode in order
        //   to get smooth streams when the stream is paused/resumed
        let launch_vid_select = match self.when_paused {
            PausedSources::None => match self.video_format {
                Some(StreamFormat::H264) => "! rtph264pay name=pay0",
                Some(StreamFormat::H265) => "! rtph265pay name=pay0",
                _ => "! fakesink",
            },
            _ => match self.video_format {
                Some(StreamFormat::H264) => "! x264enc !  rtph264pay name=pay0",
                Some(StreamFormat::H265) => "! x265enc ! rtph265pay name=pay0",
                _ => "! fakesink",
            },
        };

        // This is the part that deals with input from camera
        // - In the case of PausedSources::None it just passes the stream
        // - In the case of other Paused sources we decode it so that we can manipulate
        //   the stream smoothly and swap the source on demand
        let launch_app_src = match self.when_paused {
            PausedSources::None => {
                match self.video_format {
                    Some(StreamFormat::H264) => {
                        "! queue silent=true max-size-bytes=10485760  min-threshold-bytes=1024 ! h264parse"
                    }
                    Some(StreamFormat::H265) => {
                        "! queue silent=true  max-size-bytes=10485760  min-threshold-bytes=1024 ! h265parse"
                    }
                    _ => "",
                }
            }
            _ => {
                match self.video_format {
                    Some(StreamFormat::H264) => {
                        "! queue silent=true max-size-bytes=10485760  min-threshold-bytes=1024 ! h264parse ! avdec_h264"
                    }
                    Some(StreamFormat::H265) => {
                        "! queue silent=true  max-size-bytes=10485760  min-threshold-bytes=1024 ! h265parse ! avdec_h265"
                    }
                    _ => "",
                }
            }
        };

        // This controls the alternaive source used when in paused state
        // PausedSources::None is the exception in that it is never swapped to this state
        let launch_alt_source = match self.when_paused {
            PausedSources::TestSrc => "videotestsrc ! video/x-raw,width=896,height=512,framerate=25/1",
            PausedSources::Black => "videotestsrc pattern=black ! video/x-raw,width=896,height=512,framerate=25/1",
            PausedSources::Still => "vid_src_tee. ! imagefreeze allow-replace=true is-live=true ! video/x-raw,framerate=25/1",
            PausedSources::None => "",
        };

        // This is the final pipeline for the audio prior to rtsp
        let launch_aud_select = match self.audio_format {
            Some(StreamFormat::Adpcm(_)) | Some(StreamFormat::Aac) => {
                "! audioconvert ! rtpL16pay name=pay1"
            }
            _ => "! fakesink",
        };

        // This is the audio source from the camera. We decode to xraw always as some clients
        // like blue iris only support raw audio
        let launch_aud = match self.audio_format {
            Some(StreamFormat::Adpcm(block_size)) => format!("caps=audio/x-adpcm,layout=dvi,block_align={},channels=1,rate=8000 ! queue silent=true max-size-bytes=10485760 min-threshold-bytes=1024 ! adpcmdec", block_size), // DVI4 is converted to pcm in the appsrc
            Some(StreamFormat::Aac) => "! queue silent=true max-size-bytes=10485760 min-threshold-bytes=1024 ! aacparse ! decodebin".to_string(),
            _ => "".to_string(),
        };

        // The alternaive audio source is silence
        let launch_aud_alt = "audiotestsrc wave=silence";

        let launch_str = &vec![
            "( ",
                // Video out pipe
                "(",
                    "input-selector name=vid_inputselect",
                    launch_vid_select,
                ")",
                // Camera vid source
                "(",
                    "appsrc name=vidsrc is-live=true block=true emit-signals=false max-bytes=52428800 do-timestamp=true format=GST_FORMAT_TIME", // 50MB max size so that it won't grow to infinite if the queue blocks
                    launch_app_src,
                    // Pipe it though a tee so that the image freeze can grab it
                    "! tee name=vid_src_tee",
                    "(",
                        "vid_src_tee. ! queue silent=true  max-size-bytes=10485760  min-threshold-bytes=1024 ! vid_inputselect.sink_0",
                    ")",

                ")",
                // Alternaive vid source
                //
                //  Used during paused mode
                "(",
                    launch_alt_source,
                    "! queue silent=true  max-size-bytes=10485760  min-threshold-bytes=1024 ! vid_inputselect.sink_1",
                ")",
                // Image freeze
                // Audio pipe
                // Audio out pipe
                "(",
                    "input-selector name=aud_inputselect",
                    launch_aud_select,
                ")",
                // Camera aud source
                "(",
                    "appsrc name=audsrc is-live=true block=true emit-signals=false max-bytes=52428800 do-timestamp=true format=GST_FORMAT_TIME",
                    &launch_aud,
                    "! queue silent=true  max-size-bytes=10485760  min-threshold-bytes=1024 ! aud_inputselect.sink_0",
                ")",
                // Camera aud alt source
                "(",
                    launch_aud_alt,
                    "! queue silent=true  max-size-bytes=10485760  min-threshold-bytes=1024 ! aud_inputselect.sink_1",
                ")",
            ")"
        ]
        .join(" ");
        debug!("Gstreamer launch str: {:?}", launch_str);
        self.factory.set_launch(launch_str);
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.vidsrc.is_connected() || self.audsrc.is_connected()
    }
}

impl Default for RtspServer {
    fn default() -> RtspServer {
        Self::new().unwrap()
    }
}

impl RtspServer {
    pub(crate) fn new() -> AnyResult<RtspServer> {
        gstreamer::init().context("Gstreamer failed to initialise")?;
        let needed_elements = &[
            (
                "appsrc",
                "app (gst-plugins-base)",
                "Cannot run without an appsrc",
            ),
            (
                "audioconvert",
                "audioconvert (gst-plugins-base)",
                "Required for audio",
            ),
            ("adpcmdec", "adpcmdec", "Required for audio"),
            (
                "h264parse",
                "videoparsersbad (gst-plugins-bad)",
                "Required for certain types of camera",
            ),
            (
                "h265parse",
                "videoparsersbad (gst-plugins-bad)",
                "Required for certain types of camera",
            ),
            (
                "rtph264pay",
                "rtp (gst-plugins-good)",
                "Required for certain types of camera",
            ),
            (
                "rtph265pay",
                "rtp (gst-plugins-good)",
                "Required for certain types of camera",
            ),
            (
                "aacparse",
                "audioparsers (gst-plugins-good)",
                "Required for certain types of camera's audio",
            ),
            ("rtpL16pay", "rtp (gst-plugins-good)", "Required for audio"),
        ];
        let sometimes_needed_elements = &[
            (
                "x264enc",
                "x264 (gst-plugins-ugly)",
                "Required to paused certain cameras",
            ),
            (
                "x265enc",
                "x265 (gst-plugins-bad)",
                "Required to paused certain cameras",
            ),
            (
                "avdec_h264",
                "libav (gst-libav)",
                "Required to paused certain cameras",
            ),
            (
                "avdec_h265",
                "libav (gst-libav)",
                "Required to paused certain cameras",
            ),
            (
                "videotestsrc",
                "videotestsrc (gst-plugins-base)",
                "Required to paused certain cameras",
            ),
            (
                "imagefreeze",
                "imagefreeze (gst-plugins-good)",
                "Required to paused certain cameras",
            ),
            (
                "audiotestsrc",
                "audiotestsrc (gst-plugins-base)",
                "Required for pausing",
            ),
            (
                "decodebin",
                "playback (gst-plugins-good)",
                "Required for pausing",
            ),
        ];
        let mut fatal = false;
        for (element, plugin, msg) in needed_elements.iter() {
            if ElementFactory::find(element).is_none() {
                error!(
                    "Missing required gstreamer plugin `{}` for `{}` element. {}",
                    plugin, element, msg
                );
                fatal = true;
            }
        }

        for (element, plugin, msg) in sometimes_needed_elements.iter() {
            if ElementFactory::find(element).is_none() {
                warn!(
                    "Missing the gstreamer plugin `{}` for `{}` element. {}",
                    plugin, element, msg
                );
            }
        }

        if fatal {
            return Err(anyhow!(
                "Required Gstreamer Elements are missing. Ensure gstreamer is installed correctly"
            ));
        }
        Ok(RtspServer {
            server: GstRTSPServer::new(),
            main_loop: Arc::new(glib::MainLoop::new(None, false)),
        })
    }

    pub(crate) fn add_stream<T: AsRef<str>>(
        &self,
        paths: &[&str],
        permitted_users: &HashSet<T>,
    ) -> Result<GstOutputs> {
        let mounts = self
            .server
            .mount_points()
            .expect("The server should have mountpoints");

        // Create a MaybeAppSrc: Write which we will give the caller.  When the backing AppSrc is
        // created by the factory, fish it out and give it to the waiting MaybeAppSrc via the
        // channel it provided.  This callback may be called more than once by Gstreamer if it is
        // unhappy with the pipeline, so keep updating the MaybeAppSrc.
        let (maybe_app_src, tx) = MaybeAppSrc::new_with_tx();
        let (maybe_app_src_aud, tx_aud) = MaybeAppSrc::new_with_tx();

        let (maybe_vid_inputselect, tx_vid_inputselect) = MaybeInputSelect::new_with_tx();
        let (maybe_aud_inputselect, tx_aud_inputselect) = MaybeInputSelect::new_with_tx();

        let outputs = GstOutputs::from_appsrcs(
            maybe_app_src,
            maybe_app_src_aud,
            maybe_vid_inputselect,
            maybe_aud_inputselect,
        );

        let factory = &outputs.factory;

        debug!(
            "Permitting {} to access {}",
            // This is hashmap or (iter) equivalent of join, it requres itertools
            itertools::Itertools::intersperse(permitted_users.iter().map(|i| i.as_ref()), ", ")
                .collect::<String>(),
            paths.join(", ")
        );
        self.add_permitted_roles(factory, permitted_users);

        factory.set_shared(true);

        factory.connect_media_configure(move |_factory, media| {
            debug!("RTSP: media was configured");
            let bin = media
                .element()
                // .expect("Media should have an element")
                .dynamic_cast::<Bin>()
                .expect("Media source's element should be a bin");
            let app_src = bin
                .by_name_recurse_up("vidsrc")
                .expect("vidsrc must be present in created bin")
                .dynamic_cast::<AppSrc>()
                .expect("Source element is expected to be an appsrc!");
            let _ = tx.send(app_src); // Receiver may be dropped, don't panic if so

            let app_src_aud = bin
                .by_name_recurse_up("audsrc")
                .expect("audsrc must be present in created bin")
                .dynamic_cast::<AppSrc>()
                .expect("Source element is expected to be an appsrc!");
            let _ = tx_aud.send(app_src_aud); // Receiver may be dropped, don't panic if so

            let maybe_vid_inputselect = bin
                .by_name_recurse_up("vid_inputselect")
                .expect("vid_inputselect must be present in created bin");
            let _ = tx_vid_inputselect.send(maybe_vid_inputselect); // Receiver may be dropped, don't panic if so

            let maybe_aud_inputselect = bin
                .by_name_recurse_up("aud_inputselect")
                .expect("aud_inputselect must be present in created bin");
            let _ = tx_aud_inputselect.send(maybe_aud_inputselect); // Receiver may be dropped, don't panic if so
        });

        for path in paths {
            mounts.add_factory(path, factory);
        }

        Ok(outputs)
    }

    pub(crate) fn remove_stream(&self, paths: &[&str]) -> Result<()> {
        let mounts = self
            .server
            .mount_points()
            .expect("The server should have mountpoints");
        for path in paths {
            mounts.remove_factory(path);
        }
        Ok(())
    }

    pub(crate) fn add_permitted_roles<T: AsRef<str>>(
        &self,
        factory: &RTSPMediaFactory,
        permitted_roles: &HashSet<T>,
    ) {
        for permitted_role in permitted_roles {
            factory.add_role_from_structure(&Structure::new(
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
            factory.add_role_from_structure(&Structure::new(
                "anonymous",
                &[(*RTSP_PERM_MEDIA_FACTORY_ACCESS, &true)],
            ));
        }
    }

    pub(crate) fn set_credentials(&self, credentials: &[(&str, &str)]) -> Result<()> {
        let auth = self.server.auth().unwrap_or_else(RTSPAuth::new);
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

    pub(crate) fn set_tls(
        &self,
        cert_file: &str,
        client_auth: TlsAuthenticationMode,
    ) -> Result<()> {
        debug!("Setting up TLS using {}", cert_file);
        let auth = self.server.auth().unwrap_or_else(RTSPAuth::new);

        // We seperate reading the file and changing to a PEM so that we get different error messages.
        let cert_contents = fs::read_to_string(cert_file).expect("TLS file not found");
        let cert = TlsCertificate::from_pem(&cert_contents).expect("Not a valid TLS certificate");
        auth.set_tls_certificate(Some(&cert));
        auth.set_tls_authentication_mode(client_auth);

        self.server.set_auth(Some(&auth));
        Ok(())
    }

    pub(crate) async fn run(&self, bind_addr: &str, bind_port: u16) {
        self.server.set_address(bind_addr);
        self.server.set_service(&format!("{}", bind_port));
        // Attach server to default Glib context
        let _ = self.server.attach(None);
        let main_loop = self.main_loop.clone();
        // Run the Glib main loop.
        let _ = tokio::task::spawn_blocking(move || main_loop.run()).await;
    }

    #[allow(dead_code)]
    pub(crate) fn quit(&self) {
        self.main_loop.quit();
    }
}

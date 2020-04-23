use gstreamer::Bin;
use gstreamer::prelude::Cast;
use gstreamer_app::AppSrc;
use gstreamer_rtsp_server::{RTSPServer as GstRTSPServer, RTSPAuth, RTSPMediaFactory};
use gstreamer_rtsp_server::prelude::*;
use log::debug;
use self::maybe_app_src::MaybeAppSrc;
use std::io;
use std::io::Write;

type Result<T> = std::result::Result<T, ()>;

pub struct RtspServer {
    server: GstRTSPServer,
}

impl RtspServer {
    pub fn new() -> RtspServer {
        RtspServer {
            server: GstRTSPServer::new(),
        }
    }

    pub fn add_stream(&mut self, name: &str) -> Result<impl Write> {
        let mounts = self.server.get_mount_points().expect("The server should have mountpoints");

        let factory = RTSPMediaFactory::new();
        // TODO data from the camera may already be payloaded; in that case I am not sure what to
        // create as pay0 (maybe depay/pay is an option)
        factory.set_launch("( appsrc name=writesrc ! rtph265pay name=pay0 )");
        factory.set_shared(true);
        factory.set_stop_on_disconnect(false); // I think appsrc must be allowed to continue producing

        // TODO maybe set video format, either via caps or via
        // https://gitlab.freedesktop.org/gstreamer/gstreamer-rs/-/blob/master/examples/src/bin/appsrc.rs#L66

        // Create a MaybeAppSrc: Write which we will give the caller.  When the backing AppSrc is
        // created by the factory, fish it out and give it to the waiting MaybeAppSrc via the
        // channel it provided.
        let (maybe_app_src, tx) = MaybeAppSrc::new_with_tx();
        factory.connect_media_configure(move |_factory, media| {
            debug!("RTSP: media was configured");
            let bin = media.get_element()
                           .expect("Media should have an element")
                           .dynamic_cast::<Bin>()
                           .expect("Media source's element should be a bin");
            let app_src = bin.get_by_name_recurse_up("writesrc")
                             .expect("write_src must be present in created bin")
                             .dynamic_cast::<AppSrc>()
                             .expect("Source element is expected to be an appsrc!");
            tx.send(app_src).expect("No trouble expected sending the appsrc");
        });

        mounts.add_factory(&format!("/{}", name), &factory);

        Ok(maybe_app_src)
    }

    pub fn set_credentials(&mut self, user_pass: Option<(&str, &str)>) -> Result<()> {
        let auth = user_pass.map(|(user, pass)| {
            let auth = RTSPAuth::new();
            /*
            let perm = RTSPToken::new(
                ...
            );
            auth.add_basic(RTSPAuth::make_basic(user, pass).as_str(), &perm);
            */
            // TODO TLS https://thiblahute.github.io/GStreamer-doc/gst-rtsp-server-1.0/rtsp-server.html?gi-language=c
            auth
        });

        self.server.set_auth(auth.as_ref());

        Ok(())
    }

    pub fn run(&mut self, bind_addr: &str) {
        self.server.set_address(bind_addr);

        // Attach server to default Glib context
        self.server.attach(None);
    }
}

mod maybe_app_src {
    use super::*;
    use std::sync::mpsc::{sync_channel, SyncSender, Receiver};

    /// A Write implementation around AppSrc that also allows delaying the creation of the AppSrc
    /// until later, discarding written data until the AppSrc is provided.
    pub enum MaybeAppSrc {
        Receiver(Receiver<AppSrc>),
        AppSrc(AppSrc),
    }

    impl MaybeAppSrc {
        /// Creates a MaybeAppSrc.  Also returns a Sender that you must use to provide an AppSrc as
        /// soon as one is available.  When it is received, the MaybeAppSrc will start pushing data
        /// into the AppSrc when write() is called.
        pub fn new_with_tx() -> (Self, SyncSender<AppSrc>) {
            let (tx, rx) = sync_channel(1);
            (MaybeAppSrc::Receiver(rx), tx)
        }

        /// Attempts to retrieve the AppSrc that should be passed in by the caller of new_with_tx
        /// at some point after this struct has been created.  At that point, we swap over to
        /// owning the AppSrc directly and drop the channel.  This function handles either case and
        /// returns the AppSrc, or None if the caller has not yet sent one.
        fn try_get_src(&mut self) -> Option<&AppSrc> {
            use MaybeAppSrc::*;
            match self {
                AppSrc(ref src) => Some(src),
                Receiver(rx) => if let Some(src) = rx.try_recv().ok() {
                    *self = AppSrc(src);
                    self.try_get_src()
                } else { None }
            }
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
                let mut gst_buf_data = gst_buf.get_mut().unwrap()
                                              .map_writable().unwrap();
                gst_buf_data.copy_from_slice(buf);
            }
            app_src.push_buffer(gst_buf)
                   .map_err(|e| io::Error::new(io::ErrorKind::Other, Box::new(e)))?;
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

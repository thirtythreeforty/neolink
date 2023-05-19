// This is the streaming state
//
// Data is streamed into a gstreamer source

use anyhow::{anyhow, Context, Error, Result};
use log::*;
use neolink_core::bcmedia::model::BcMedia;
use tokio::time::{timeout, Duration};
use tokio::{sync::RwLock, task::JoinSet};

use neolink_core::bc_protocol::{BcCamera, StreamKind as Stream};

use super::{camera::Camera, LoggedIn};
use crate::rtsp::gst::FactoryCommand;

pub(crate) struct Streaming {
    pub(crate) camera: BcCamera,
    set: RwLock<JoinSet<Result<()>>>,
}

impl Camera<Streaming> {
    pub(crate) async fn from_login(loggedin: Camera<LoggedIn>) -> Result<Camera<Streaming>, Error> {
        // Create new gst outputs
        //
        // Otherwise use those already present
        let mut set: JoinSet<Result<()>> = Default::default();
        for stream in loggedin.shared.streams.iter() {
            let tag = loggedin.shared.get_tag_for_stream(stream);
            let sender = loggedin
                .shared
                .rtsp
                .get_sender(&tag)
                .await
                .ok_or_else(|| anyhow!("Stream has not been created"))?;

            let stream_display_name = match stream {
                Stream::Main => "Main Stream (Clear)",
                Stream::Sub => "Sub Stream (Fluent)",
                Stream::Extern => "Extern Stream (Balanced)",
            };
            info!(
                "{}: Starting video stream {}",
                &loggedin.shared.config.name, stream_display_name
            );

            let stream_thead = *stream;
            let strict_thread = loggedin.shared.config.strict;
            let tag_thread = tag.clone();
            let mut stream_data = loggedin
                .state
                .camera
                .start_video(stream_thead, 0, strict_thread)
                .await?;
            set.spawn(async move {
                loop {
                    tokio::task::yield_now().await;
                    // debug!("Straming: Get");
                    let data = timeout(Duration::from_secs(15), stream_data.get_data())
                        .await
                        .with_context(|| "Timed out waiting for new Media Frame")??;
                    // debug!("Straming: Got");
                    match &data {
                        Ok(BcMedia::InfoV1(_)) => trace!("{}:  - InfoV1", &tag_thread),
                        Ok(BcMedia::InfoV2(_)) => trace!("{}:  - InfoV2", &tag_thread),
                        Ok(BcMedia::Iframe(_)) => trace!("{}:  - Iframe", &tag_thread),
                        Ok(BcMedia::Pframe(_)) => trace!("{}:  - Pframe", &tag_thread),
                        Ok(BcMedia::Aac(_)) => trace!("{}:  - Aac", &tag_thread),
                        Ok(BcMedia::Adpcm(_)) => trace!("{}:  - Adpcm", &tag_thread),
                        Err(_) => trace!("  - Error"),
                    }
                    // debug!("Straming: Send");
                    timeout(
                        Duration::from_secs(15),
                        sender.send(FactoryCommand::BcMedia(data?)),
                    )
                    .await
                    .with_context(|| "Timed out waiting to send Media Frame")??;
                    // debug!("Straming: Sent");
                }
            });
        }

        Ok(Camera {
            shared: loggedin.shared,
            state: Streaming {
                camera: loggedin.state.camera,
                set: RwLock::new(set),
            },
        })
    }

    pub(crate) async fn stop(self) -> Result<Camera<LoggedIn>> {
        self.state.set.into_inner().abort_all();
        Ok(Camera {
            shared: self.shared,
            state: LoggedIn {
                camera: self.state.camera,
            },
        })
    }

    pub(crate) async fn join(&self) -> Result<()> {
        tokio::select! {
            v = async {
                let mut locked_threads = self.state.set.write().await;
                while let Some(res) = locked_threads.join_next().await {
                    match res {
                        Err(e) => {
                            locked_threads.abort_all();
                            return Err(e.into());
                        }
                        Ok(Err(e)) => {
                            locked_threads.abort_all();
                            return Err(e);
                        }
                        Ok(Ok(())) => {}
                    }
                }
                Ok(())
            } => v,
            v = self.state.camera.join() => v.map_err(|e| anyhow!("Camera join error: {:?}", e)),
        }?;
        Ok(())
    }

    pub(crate) fn get_camera(&self) -> &BcCamera {
        &self.state.camera
    }
}

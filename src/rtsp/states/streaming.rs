// This is the streaming state
//
// Data is streamed into a gstreamer source

use std::collections::HashMap;

use futures::stream::{FuturesUnordered, StreamExt};

use anyhow::{anyhow, Context, Error, Result};
use log::*;
use neolink_core::bcmedia::model::BcMedia;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use neolink_core::bc_protocol::{BcCamera, StreamKind as Stream};

use super::{camera::Camera, LoggedIn};
use crate::rtsp::gst::FactoryCommand;

pub(crate) struct Streaming {
    pub(crate) camera: BcCamera,
    stream_handles: RwLock<HashMap<Stream, JoinHandle<Result<()>>>>,
}

impl Camera<Streaming> {
    pub(crate) async fn from_login(loggedin: Camera<LoggedIn>) -> Result<Camera<Streaming>, Error> {
        // Create new gst outputs
        //
        // Otherwise use those already present
        let mut me = Camera {
            shared: loggedin.shared,
            state: Streaming {
                camera: loggedin.state.camera,
                stream_handles: Default::default(),
            },
        };

        for stream in me.shared.streams.iter().copied().collect::<Vec<_>>() {
            me.start_stream(stream).await?;
        }

        Ok(me)
    }

    pub(crate) async fn start_stream(&mut self, stream: Stream) -> Result<()> {
        if let Some(handle) = self.state.stream_handles.get_mut().get(&stream) {
            if !handle.is_finished() {
                return Ok(());
            }
        }
        let tag = self.shared.get_tag_for_stream(&stream);
        let sender = self
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
            &self.shared.config.name, stream_display_name
        );

        let stream_thead = stream;
        let strict_thread = self.shared.config.strict;
        let tag_thread = tag.clone();
        let mut stream_data = self
            .state
            .camera
            .start_video(stream_thead, 0, strict_thread)
            .await?;
        let handle = tokio::task::spawn(async move {
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
        self.state.stream_handles.get_mut().insert(stream, handle);
        Ok(())
    }

    pub(crate) async fn stop_stream(&mut self, stream: Stream) -> Result<()> {
        if let Some(handle) = self.state.stream_handles.get_mut().remove(&stream) {
            handle.abort();
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn stop(self) -> Result<Camera<LoggedIn>> {
        self.state
            .stream_handles
            .into_inner()
            .into_values()
            .for_each(|h| h.abort());
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
                let mut locked_threads = self.state.stream_handles.write().await;
                let mut thread_joins = locked_threads.iter_mut().map(|(_, t)| t).collect::<FuturesUnordered::<_>>();

                while let Some(res) = thread_joins.next().await {
                    match res {
                        Err(e) => {
                            drop(thread_joins);
                            locked_threads.iter_mut().for_each(|(_,h)| h.abort());
                            return Err(e.into());
                        }
                        Ok(Err(e)) => {
                            drop(thread_joins);
                            locked_threads.iter_mut().for_each(|(_,h)| h.abort());
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

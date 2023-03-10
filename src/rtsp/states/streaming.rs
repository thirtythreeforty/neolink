// This is the streaming state
//
// Data is streamed into a gstreamer source

use anyhow::{anyhow, Context, Error, Result};
use async_trait::async_trait;
use log::*;
use std::collections::HashMap;
use tokio::task::{self, JoinHandle};

use neolink_core::bc_protocol::StreamKind as Stream;

use super::{CameraState, Shared};

use crate::rtsp::abort::AbortHandle;

#[derive(Default)]
pub(crate) struct Streaming {
    handles: HashMap<Stream, JoinHandle<Result<(), Error>>>,
    abort_handle: AbortHandle,
}

#[async_trait]
impl CameraState for Streaming {
    async fn setup(&mut self, shared: &Shared) -> Result<(), Error> {
        self.abort_handle.reset();
        // Create new gst outputs
        //
        // Otherwise use those already present
        for stream in shared.streams.iter() {
            let tag = shared.get_tag_for_stream(stream);
            let sender = shared
                .rtsp
                .get_sender(tag)
                .await
                .ok_or_else(|| anyhow!("Stream has not been created"))?;

            let stream_display_name = match stream {
                Stream::Main => "Main Stream (Clear)",
                Stream::Sub => "Sub Stream (Fluent)",
                Stream::Extern => "Extern Stream (Balanced)",
            };
            info!(
                "{}: Starting video stream {}",
                &shared.name, stream_display_name
            );

            let thread_camera = shared.camera.clone();
            let thread_abort_handle = self.abort_handle.clone();
            let stream_thead = *stream;
            let strict_thread = shared.strict;
            let handle = task::spawn(async move {
                let mut stream_data = thread_camera
                    .start_video(stream_thead, 0, strict_thread)
                    .await?;
                while thread_abort_handle.is_live() {
                    let data = stream_data.get_data().await?;
                    sender.send(data?).await?;
                }
                Ok(())
            });

            self.handles.entry(*stream).or_insert_with(|| handle);
        }

        Ok(())
    }

    async fn tear_down(&mut self, _shared: &Shared) -> Result<(), Error> {
        self.abort_handle.abort();

        if !self.handles.is_empty() {
            for (stream, handle) in self.handles.drain() {
                match handle.await {
                    Ok(Err(e)) => return Err(e),
                    Err(_) => return Err(anyhow!("Panicked while streaming {:?}", stream)),
                    Ok(Ok(_)) => {}
                }
            }
        }

        Ok(())
    }
}

impl Drop for Streaming {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}

impl Streaming {
    pub(crate) async fn is_running(&mut self) -> Result<()> {
        if self.handles.iter().all(|(_, h)| !h.is_finished()) && self.abort_handle.is_live() {
            return Ok(());
        }
        if !self.abort_handle.is_live() {
            return Err(anyhow!("Stream aborted"));
        }
        for (s, h) in self.handles.drain() {
            h.await?.context(format!("On stream: {:?}", s))?;
        }
        Ok(())
    }
}

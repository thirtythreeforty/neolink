// This is the streaming state
//
// Data is streamed into a gstreamer source

use anyhow::{anyhow, Error, Result};
use async_trait::async_trait;
use futures::future::FutureExt;
use log::*;
use tokio::task::JoinSet;
use tokio::time::{timeout, Duration};

use neolink_core::{bc_protocol::StreamKind as Stream, bcmedia::model::BcMedia};

use super::{CameraState, Shared};

#[derive(Default)]
pub(crate) struct Streaming {
    set: JoinSet<Result<()>>,
}

#[async_trait]
impl CameraState for Streaming {
    async fn setup(&mut self, shared: &Shared) -> Result<(), Error> {
        // Create new gst outputs
        //
        // Otherwise use those already present
        for stream in shared.streams.iter() {
            let tag = shared.get_tag_for_stream(stream);
            let sender = shared
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
                &shared.name, stream_display_name
            );

            let thread_camera = shared.camera.clone();
            let stream_thead = *stream;
            let strict_thread = shared.strict;
            let tag_thread = tag.clone();
            self.set.spawn(async move {
                let mut stream_data = thread_camera
                    .start_video(stream_thead, 0, strict_thread)
                    .await?;
                loop {
                    debug!("{}: BcMediaStreamRecv", &tag_thread);
                    let data = timeout(Duration::from_secs(15), stream_data.get_data()).await??;
                    match &data {
                        Ok(BcMedia::InfoV1(_)) => debug!("{}:  - InfoV1", &tag_thread),
                        Ok(BcMedia::InfoV2(_)) => debug!("{}:  - InfoV2", &tag_thread),
                        Ok(BcMedia::Iframe(_)) => debug!("{}:  - Iframe", &tag_thread),
                        Ok(BcMedia::Pframe(_)) => debug!("{}:  - Pframe", &tag_thread),
                        Ok(BcMedia::Aac(_)) => debug!("{}:  - Aac", &tag_thread),
                        Ok(BcMedia::Adpcm(_)) => debug!("{}:  - Adpcm", &tag_thread),
                        Err(_) => debug!("  - Error"),
                    }
                    sender.send(data?).await?;
                }
            });
        }

        Ok(())
    }

    async fn tear_down(&mut self, _shared: &Shared) -> Result<(), Error> {
        self.set.shutdown().await;
        Ok(())
    }
}

impl Drop for Streaming {
    fn drop(&mut self) {
        self.set.abort_all();
    }
}

impl Streaming {
    pub(crate) async fn is_running(&mut self) -> Result<()> {
        if self.set.is_empty() {
            return Err(anyhow!("Streaming has no active tasks"));
        }
        if let Some(res) = self.set.join_next().now_or_never() {
            match res {
                None => {
                    return Err(anyhow!("Streaming has no active tasks"));
                }
                Some(Ok(Ok(()))) => {
                    unreachable!();
                }
                Some(Ok(Err(e))) => {
                    // Error in tasks
                    self.set.abort_all();
                    return Err(e);
                }
                Some(Err(e)) => {
                    // Panic in tasks
                    self.set.abort_all();
                    return Err(anyhow!("Panic in streaming task: {:?}", e));
                }
            }
        }
        Ok(())
    }
}

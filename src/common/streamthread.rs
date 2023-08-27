//! This thread will start and stop the camera
//! based on the number of listeners to the
//! async streams

use std::collections::{hash_map::Entry, HashMap};
use tokio::sync::{
    broadcast::{channel as broadcast, Sender as BroadcastSender},
    mpsc::Receiver as MpscReceiver,
    oneshot::Sender as OneshotSender,
};
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::sync::CancellationToken;

use super::NeoInstance;
use crate::Result;
use neolink_core::{bc_protocol::StreamKind, bcmedia::model::BcMedia};

pub(crate) struct NeoCamStreamThread {
    streams: HashMap<StreamKind, StreamData>,
    stream_request_rx: MpscReceiver<StreamRequest>,
    cancel: CancellationToken,
    instance: NeoInstance,
}

impl NeoCamStreamThread {
    pub(crate) async fn new(
        stream_request_rx: MpscReceiver<StreamRequest>,
        instance: NeoInstance,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            streams: Default::default(),
            stream_request_rx,
            cancel,
            instance,
        }
    }
    pub(crate) async fn run(&mut self) -> Result<()> {
        let thread_cancel = self.cancel.clone();
        tokio::select! {
            _ = thread_cancel.cancelled() => Ok(()),
            v = async {
                while let Some(request) = self.stream_request_rx.recv().await {
                    match self.streams.entry(request.name) {
                        Entry::Occupied(occ) => {
                            let _ = request
                                .sender
                                .send(BroadcastStream::new(occ.get().sender.subscribe()));
                        }
                        Entry::Vacant(vac) => {
                            // Make a new streaming instance

                            let (sender, stream_rx) = broadcast(1000);
                            let data = StreamData {
                                sender,
                                name: request.name,
                                instance: self.instance.subscribe().await?,
                                strict: request.strict,
                                cancel: CancellationToken::new(),
                            };
                            data.run().await?;
                            vac.insert(data);
                            let _ = request.sender.send(BroadcastStream::new(stream_rx));
                        }
                    }
                }
                Ok(())
            } => v,
        }
    }
}

/// The kind of stream we want a async broadcast of
pub(crate) struct StreamRequest {
    pub(crate) name: StreamKind,
    pub(crate) sender: OneshotSender<BroadcastStream<BcMedia>>,
    pub(crate) strict: bool,
}

/// The data of a running stream
pub(crate) struct StreamData {
    sender: BroadcastSender<BcMedia>,
    name: StreamKind,
    instance: NeoInstance,
    strict: bool,
    cancel: CancellationToken,
}

impl StreamData {
    async fn run(&self) -> Result<()> {
        let thread_stream_tx = self.sender.clone();
        let cancel = self.cancel.clone();
        let instance = self.instance.subscribe().await?;
        let name = self.name;
        let strict = self.strict;
        tokio::task::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {
                    Result::<(), anyhow::Error>::Ok(())
                },
                v = async {
                    loop {
                        instance.run_task(|camera| {
                            let stream_tx = thread_stream_tx.clone();
                            Box::pin(async move {
                                let mut stream_data = camera.start_video(name, 0, strict).await?;
                                loop {
                                    let data = stream_data.get_data().await??;
                                    stream_tx.send(data)?;
                                }
                            })
                        }).await?;
                    }
                }    => v,
            }
        });

        Ok(())
    }
}

impl Drop for StreamData {
    fn drop(&mut self) {
        log::debug!("Cancel:: StreamData::drop");
        self.cancel.cancel();
    }
}

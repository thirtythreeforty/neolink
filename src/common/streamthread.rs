//! This thread will start and stop the camera
//!
//! If there are no listeners to the broadcast
//! then it will hangup

use std::collections::{hash_map::Entry, HashMap};
use tokio::{
    sync::{
        broadcast::{channel as broadcast, Sender as BroadcastSender},
        mpsc::Receiver as MpscReceiver,
        oneshot::Sender as OneshotSender,
    },
    task::JoinHandle,
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
                        Entry::Occupied(mut occ) => {
                            occ.get_mut().ensure_running().await?;
                            let _ = request
                                .sender
                                .send(BroadcastStream::new(occ.get().sender.subscribe()));
                        }
                        Entry::Vacant(vac) => {
                            // Make a new streaming instance

                            let (sender, stream_rx) = broadcast(1000);
                            let data = StreamData::new(
                                sender,
                                request.name,
                                self.instance.subscribe().await?,
                                request.strict,
                            ).await?;
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

impl Drop for NeoCamStreamThread {
    fn drop(&mut self) {
        self.cancel.cancel();
        for stream in self.streams.values() {
            stream.cancel.cancel()
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
    cancel: CancellationToken,
    handle: Option<JoinHandle<Result<()>>>,
    strict: bool,
}

impl StreamData {
    async fn new(
        sender: BroadcastSender<BcMedia>,
        name: StreamKind,
        instance: NeoInstance,
        strict: bool,
    ) -> Result<Self> {
        let mut me = Self {
            name,
            cancel: CancellationToken::new(),
            sender,
            instance,
            handle: None,
            strict,
        };

        me.restart().await?;

        Ok(me)
    }

    async fn ensure_running(&mut self) -> Result<()> {
        if self.cancel.is_cancelled()
            || self
                .handle
                .as_ref()
                .map(|handle| handle.is_finished())
                .unwrap_or(true)
        {
            log::debug!("Restart stream");
            self.restart().await?;
        }
        Ok(())
    }

    async fn restart(&mut self) -> Result<()> {
        self.shutdown().await?;
        self.cancel = CancellationToken::new();

        let cancel = self.cancel.clone();
        let sender = self.sender.clone();
        let instance = self.instance.subscribe().await?;
        let name = self.name;
        let strict = self.strict;
        self.handle = Some(tokio::task::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {
                    Result::<(), anyhow::Error>::Ok(())
                },
                v = async {
                    loop {
                        let result = instance.run_task(|camera| {
                            let stream_tx = sender.clone();
                            Box::pin(async move {
                                let res = async {
                                    let mut stream_data = camera.start_video(name, 0, strict).await?;
                                    loop {
                                        let data = stream_data.get_data().await??;
                                        if stream_tx.send(data).is_err() {
                                            // If noone is listening for the stream we error and stop here
                                            break;
                                        };
                                    }
                                    Result::<(),anyhow::Error>::Ok(())
                                }.await;
                                Ok(res)
                            })
                        }).await;
                        match result {
                            Ok(Ok(())) => {
                                log::debug!("Video Stream Stopped due to no listeners");
                                break;
                            },
                            Ok(Err(e)) => {
                                log::debug!("Video Stream Restarting Due to Error: {:?}", e);
                            },
                            Err(e) => {
                                log::debug!("Video Stream Stopped Due to Instance Error: {:?}", e);
                                break;
                            },
                        }
                    }
                    Ok(())
                }    => v,
            }
        }));

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.cancel.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        Ok(())
    }
}

impl Drop for StreamData {
    fn drop(&mut self) {
        log::debug!("Cancel:: StreamData::drop");
        self.cancel.cancel();
    }
}

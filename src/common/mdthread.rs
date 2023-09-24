//! This thread will listen to motion messages
//! from the camera.

use std::sync::Arc;
use tokio::{
    sync::{
        mpsc::Receiver as MpscReceiver,
        oneshot::Sender as OneshotSender,
        watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
    },
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::NeoInstance;
use crate::Result;
use neolink_core::bc_protocol::MotionStatus;

pub(crate) enum MdState {
    Start(Instant),
    Stop(Instant),
    Unknown,
}

pub(crate) struct NeoCamMdThread {
    md_watcher: Arc<WatchSender<MdState>>,
    md_request_rx: MpscReceiver<MdRequest>,
    cancel: CancellationToken,
    instance: NeoInstance,
}

impl NeoCamMdThread {
    pub(crate) async fn new(
        md_request_rx: MpscReceiver<MdRequest>,
        instance: NeoInstance,
    ) -> Result<Self> {
        let (md_watcher, _) = watch(MdState::Unknown);
        let md_watcher = Arc::new(md_watcher);
        Ok(Self {
            md_watcher,
            md_request_rx,
            cancel: CancellationToken::new(),
            instance,
        })
    }

    pub(crate) async fn run(&mut self) -> Result<()> {
        let thread_cancel = self.cancel.clone();
        let watcher = self.md_watcher.clone();
        let md_instance = self.instance.clone();
        tokio::select! {
            _ = thread_cancel.cancelled() => {
                Ok(())
            },
            v = async {
                while let Some(request) = self.md_request_rx.recv().await {
                    match request {
                        MdRequest::Get {
                            sender
                        } => {
                          let _ = sender.send(self.md_watcher.subscribe());
                        },
                    }
                }
                Ok(())
            } => v,
            v = md_instance.run_passive_task(|cam| {
                    let watcher = watcher.clone();
                    Box::pin(
                    async move {
                        let mut md = cam.listen_on_motion().await?;
                        loop {
                            let event = md.next_motion().await?;
                            match event {
                                MotionStatus::Start(at) => {
                                    watcher.send_replace(
                                        MdState::Start(at.into())
                                    );
                                }
                                MotionStatus::Stop(at) => {
                                    watcher.send_replace(
                                        MdState::Stop(at.into())
                                    );
                                }
                                MotionStatus::NoChange(_) => {},
                            }
                        }
                    }
                )}) => v
        }
    }
}

impl Drop for NeoCamMdThread {
    fn drop(&mut self) {
        log::trace!("Drop NeoCamMdThread");
        self.cancel.cancel();
        log::trace!("Dropped NeoCamMdThread");
    }
}

/// Used to pass messages to the MdThread
pub(crate) enum MdRequest {
    Get {
        sender: OneshotSender<WatchReceiver<MdState>>,
    },
}

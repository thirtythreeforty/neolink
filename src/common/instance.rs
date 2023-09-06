//! The sharable instance
//!
//! This communicaes with the [`NeoCam`] over channels
//!
//! The camera watch is used as an event to be triggered
//! whenever the camera is lost/updated
use anyhow::anyhow;
use std::sync::Weak;
use tokio::sync::{
    mpsc::Sender as MpscSender, oneshot::channel as oneshot, watch::Receiver as WatchReceiver,
};
use tokio_util::sync::CancellationToken;

use super::{MdState, NeoCamCommand, StreamInstance};
use crate::{config::CameraConfig, Result};
use neolink_core::bc_protocol::{BcCamera, StreamKind};

/// This instance is the primary interface used throughout the app
///
/// It uses channels to run all tasks on the actual shared `[NeoCam]`
#[derive(Clone)]
pub(crate) struct NeoInstance {
    camera_watch: WatchReceiver<Weak<BcCamera>>,
    camera_control: MpscSender<NeoCamCommand>,
    cancel: CancellationToken,
}

impl NeoInstance {
    pub(crate) fn new(
        camera_watch: WatchReceiver<Weak<BcCamera>>,
        camera_control: MpscSender<NeoCamCommand>,
        cancel: CancellationToken,
    ) -> Result<Self> {
        Ok(Self {
            camera_watch,
            camera_control,
            cancel,
        })
    }

    /// Create a new instance to the same camera
    ///
    /// Unlike clone this one will contact the NeoCam and grab it from
    /// there. There is no real benifit to this, other then being
    /// able to check if the thread is alive. Which is why it can
    /// fail.
    pub(crate) async fn subscribe(&self) -> Result<Self> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::Instance(instance_tx))
            .await?;
        instance_rx.await?
    }

    pub(crate) async fn shutdown(&self) {
        let _ = self.camera_control.send(NeoCamCommand::HangUp).await;
        self.cancel.cancelled().await
    }

    /// This is a helpful convience function
    ///
    /// Given an async task it will:
    /// - Run the task with a reference to a BcCamera
    /// - If the camera instance is changed: Rerun the task with the new instance
    /// - If the camera returns a retryable error, wait for camera instance to change then rerun
    /// - else return the result of the function
    pub(crate) async fn run_task<F, T>(&self, task: F) -> Result<T>
    where
        F: for<'a> Fn(
            &'a BcCamera,
        )
            -> std::pin::Pin<Box<dyn futures::Future<Output = Result<T>> + Send + 'a>>,
    {
        let mut camera_watch = self.camera_watch.clone();
        let mut camera = camera_watch.borrow_and_update().upgrade();

        loop {
            let res = tokio::select! {
                _ = self.cancel.cancelled() => {
                    Some(Err(anyhow!("Camera is disconnecting")))
                }
                v = camera_watch.changed() => {
                    // Camera value has changed!
                    // update and try again
                    if v.is_ok() {
                        camera = camera_watch.borrow_and_update().upgrade();
                        None
                    } else {
                        Some(Err(anyhow!("Camera is disconnecting")))
                    }
                },
                Some(v) = async {
                    if let Some(cam) = camera.clone() {
                        let cam_ref = cam.as_ref();
                        Some(task(cam_ref).await)
                    } else {
                        None
                    }
                }, if camera.is_some() => {
                    match v {
                        // Ok means we are done
                        Ok(v) => Some(Ok(v)),
                        // If error we check for retryable errors
                        Err(e) => {
                            match e.downcast::<neolink_core::Error>() {
                                // Retry is a None
                                Ok(neolink_core::Error::DroppedConnection) => {
                                    camera = None;
                                    None
                                },
                                Ok(e) => Some(Err(e.into())),
                                Err(e) => Some(Err(e)),
                            }
                        }
                    }
                },
            };

            if let Some(res) = res {
                return res;
            }
        }
    }

    pub(crate) async fn stream(&self, name: StreamKind) -> Result<StreamInstance> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::Stream(name, instance_tx))
            .await?;
        Ok(instance_rx.await?)
    }

    #[allow(dead_code)]
    pub(crate) async fn low_stream(&self) -> Result<Option<StreamInstance>> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::LowStream(instance_tx))
            .await?;
        Ok(instance_rx.await?)
    }

    #[allow(dead_code)]
    pub(crate) async fn high_stream(&self) -> Result<Option<StreamInstance>> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::HighStream(instance_tx))
            .await?;
        Ok(instance_rx.await?)
    }

    #[allow(dead_code)]
    pub(crate) async fn streams(&self) -> Result<Vec<StreamInstance>> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::Streams(instance_tx))
            .await?;
        Ok(instance_rx.await?)
    }

    pub(crate) async fn motion(&self) -> Result<WatchReceiver<MdState>> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::Motion(instance_tx))
            .await?;
        Ok(instance_rx.await?)
    }

    pub(crate) async fn config(&self) -> Result<WatchReceiver<CameraConfig>> {
        let (instance_tx, instance_rx) = oneshot();
        self.camera_control
            .send(NeoCamCommand::Config(instance_tx))
            .await?;
        Ok(instance_rx.await?)
    }
}

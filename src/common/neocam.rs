//! This is the common code for creating a camera instance
//!
//! Features:
//!    Shared stream BC delivery
//!    Common restart code
//!    Clonable interface to share amongst threadsanyhow::anyhow;
use futures::stream::StreamExt;
use std::sync::Weak;
use tokio::sync::{
    mpsc::{channel as mpsc, Sender as MpscSender},
    oneshot::{channel as oneshot, Sender as OneshotSender},
    watch::{channel as watch, Receiver as WatchReceiver, Sender as WatchSender},
};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_util::sync::CancellationToken;

use super::{NeoCamStreamThread, NeoCamThread, NeoInstance, StreamInstance, StreamRequest};
use crate::{config::CameraConfig, AnyResult, Result};
use neolink_core::bc_protocol::{BcCamera, StreamKind};

pub(crate) enum NeoCamCommand {
    HangUp,
    Instance(OneshotSender<Result<NeoInstance>>),
    Stream(StreamKind, OneshotSender<StreamInstance>),
    HighStream(OneshotSender<Option<StreamInstance>>),
    LowStream(OneshotSender<Option<StreamInstance>>),
    Streams(OneshotSender<Vec<StreamInstance>>),
    Config(OneshotSender<WatchReceiver<CameraConfig>>),
}
/// The underlying camera binding
pub(crate) struct NeoCam {
    cancel: CancellationToken,
    config_watch: WatchSender<CameraConfig>,
    commander: MpscSender<NeoCamCommand>,
    camera_watch: WatchReceiver<Weak<BcCamera>>,
}

impl NeoCam {
    pub(crate) async fn new(config: CameraConfig) -> Result<NeoCam> {
        let (commander_tx, commander_rx) = mpsc(100);
        let (watch_config_tx, watch_config_rx) = watch(config.clone());
        let (camera_watch_tx, camera_watch_rx) = watch(Weak::new());
        let (stream_request_tx, stream_request_rx) = mpsc(100);

        let me = Self {
            cancel: CancellationToken::new(),
            config_watch: watch_config_tx,
            commander: commander_tx.clone(),
            camera_watch: camera_watch_rx.clone(),
        };

        // This thread recieves messages from the instances
        // and acts on it.
        //
        // This thread must be started first so that we can begin creating instances for the
        // other threads
        let sender_cancel = me.cancel.clone();
        let mut commander_rx = ReceiverStream::new(commander_rx);
        let strict = config.strict;
        let thread_commander_tx = commander_tx.clone();
        let thread_watch_config_rx = watch_config_rx.clone();
        tokio::task::spawn(async move {
            let thread_cancel = sender_cancel.clone();
            let res = tokio::select! {
                _ = sender_cancel.cancelled() => Result::Ok(()),
                v = async {
                    while let Some(command) = commander_rx.next().await {
                        match command {
                            NeoCamCommand::HangUp => {
                                sender_cancel.cancel();
                                log::debug!("Cancel:: NeoCamCommand::HangUp");
                                return Result::<(), anyhow::Error>::Ok(());
                            }
                            NeoCamCommand::Instance(result) => {
                                let instance = NeoInstance::new(
                                    camera_watch_rx.clone(),
                                    thread_commander_tx.clone(),
                                    thread_cancel.clone(),
                                );
                                let _ = result.send(instance);
                            }
                            NeoCamCommand::Stream(name, sender) => {
                                stream_request_tx.send(
                                    StreamRequest::GetOrInsert {
                                        name,
                                        sender,
                                        strict,
                                    }
                                ).await?;
                            },
                            NeoCamCommand::HighStream(sender) => {
                                stream_request_tx.send(
                                    StreamRequest::High {
                                        sender,
                                    }
                                ).await?;
                            },
                            NeoCamCommand::LowStream(sender) => {
                                stream_request_tx.send(
                                    StreamRequest::Low {
                                        sender,
                                    }
                                ).await?;
                            },
                            NeoCamCommand::Streams(sender) => {
                                stream_request_tx.send(
                                    StreamRequest::All {
                                        sender,
                                    }
                                ).await?;
                            },
                            NeoCamCommand::Config(sender) => {
                                let _ = sender.send(thread_watch_config_rx.clone());
                            }
                        }
                    }
                    Ok(())
                } => v
            };
            log::debug!("Control thread terminated");
            res
        });

        // This gets the first instance which we use for making the other threads
        let (instance_tx, instance_rx) = oneshot();
        commander_tx
            .send(NeoCamCommand::Instance(instance_tx))
            .await?;
        let instance = instance_rx.await??;

        // This thread maintains the camera loop
        //
        // It will keep it logged and reconnect
        let thread_watch_config_rx = watch_config_rx.clone();
        let mut cam_thread =
            NeoCamThread::new(thread_watch_config_rx, camera_watch_tx, me.cancel.clone()).await;
        tokio::task::spawn(async move { cam_thread.run().await });

        // This thread maintains the streams
        let stream_instance = instance.subscribe().await?;
        let stream_cancel = me.cancel.clone();
        let mut stream_thread =
            NeoCamStreamThread::new(stream_request_rx, stream_instance, stream_cancel).await?;
        tokio::task::spawn(async move { stream_thread.run().await });

        // This thread just does a one time report on camera info
        let report_instance = instance.subscribe().await?;
        let report_cancel = me.cancel.clone();
        let report_name = config.name.clone();
        tokio::task::spawn(async move {
            tokio::select! {
                _ = report_cancel.cancelled() => {
                    AnyResult::Ok(())
                }
                v = async {
                    let version = report_instance.run_task(|cam| Box::pin(
                        async move {
                            Ok(cam.version().await?)
                        }
                    )).await?;
                    log::info!("{}: Model {}", report_name, version.model.unwrap_or("Undeclared".to_string()));
                    log::info!("{}: Firmware Version {}", report_name, version.firmwareVersion);

                    let stream_info = report_instance.run_task(|cam| Box::pin(
                        async move {
                            Ok(cam.get_stream_info().await?)
                        }
                    )).await?;
                    let mut supported_streams = vec![];
                    for encode in stream_info.stream_infos.iter().flat_map(|stream_info| stream_info.encode_tables.clone()) {
                        supported_streams.push(std::format!("    {}: {}x{}", encode.name, encode.resolution.width, encode.resolution.height));
                    }
                    log::debug!("{}: Listing Camera Supported Streams\n{}", report_name, supported_streams.join("\n"));


                    Ok(())
                } => v
            }
        });

        Ok(me)
    }

    pub(crate) async fn subscribe(&self) -> Result<NeoInstance> {
        NeoInstance::new(
            self.camera_watch.clone(),
            self.commander.clone(),
            self.cancel.clone(),
        )
    }

    pub(crate) async fn update_config(&self, config: CameraConfig) -> Result<()> {
        self.config_watch.send(config)?;
        Ok(())
    }

    pub(crate) async fn shutdown(&self) {
        let _ = self.commander.send(NeoCamCommand::HangUp).await;
        self.cancel.cancelled().await
    }
}

impl Drop for NeoCam {
    fn drop(&mut self) {
        log::debug!("Cancel:: NeoCam::drop");
        self.cancel.cancel();
    }
}

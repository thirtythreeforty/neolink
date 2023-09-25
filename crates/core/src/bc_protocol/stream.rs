use super::{BcCamera, Error, Result};
use crate::{
    bc::{model::*, xml::*},
    bcmedia::model::*,
};
use futures::stream::StreamExt;
use tokio::sync::mpsc::{channel, Receiver};
use tokio::task::{self, JoinHandle};
use tokio_util::sync::CancellationToken;

/// The stream names supported by BC
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum StreamKind {
    /// This is the HD stream
    Main,
    /// This is the SD stream
    Sub,
    /// This stream represents a balance between SD and HD
    ///
    /// It is only avaliable on some camera. If the camera dosen't
    /// support it the stream will be the same as the SD stream
    Extern,
}

impl std::fmt::Display for StreamKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            StreamKind::Main => write!(f, "mainStream"),
            StreamKind::Sub => write!(f, "subStream"),
            StreamKind::Extern => write!(f, "externStream"),
        }
    }
}

/// A handle on currently streaming data
///
/// The data can be pulled using `get_data` which returns raw BcMedia packets
///
/// When this object is dropped the streaming is stopped
pub struct StreamData {
    handle: Option<JoinHandle<Result<()>>>,
    rx: Receiver<Result<BcMedia>>,
    abort_handle: CancellationToken,
}

impl StreamData {
    /// Pull data from the camera's buffer
    /// This returns raw BcMedia packets
    pub async fn get_data(&mut self) -> Result<Result<BcMedia>> {
        if let Some(handle) = self.handle.as_mut() {
            if handle.is_finished() {
                log::debug!("SteamData::get_data Cancel1");
                self.abort_handle.cancel();
                handle.await??;
                return Err(Error::DroppedConnection);
            }
        } else {
            log::debug!("SteamData::get_data Cancel2");
            self.abort_handle.cancel();
            return Err(Error::DroppedConnection);
        }
        // debug!("StreamData: Get");
        match self.rx.recv().await {
            Some(data) => {
                // debug!("StreamData: Got");
                Ok(data)
            }
            None => {
                // debug!("StreamData: Drop");
                log::debug!("SteamData::get_data Cancel3");
                self.abort_handle.cancel();
                Err(Error::DroppedConnection)
            }
        }
    }

    /// Attempts to gracefully shutdown this will cancel the background task and send
    /// the Stop command to the camera
    pub async fn shutdown(&mut self) -> Result<()> {
        log::debug!("SteamData::shutdown Cancel");
        self.abort_handle.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.await?;
        }
        Ok(())
    }
}

impl Drop for StreamData {
    fn drop(&mut self) {
        log::trace!("Drop StreamData");
        self.abort_handle.cancel();
        if let Some(handle) = self.handle.take() {
            let _gt = tokio::runtime::Handle::current().enter();
            tokio::task::spawn(async move {
                let _ = handle.await;
            });
        }
        log::trace!("Dropped MotionData");
    }
}

impl BcCamera {
    ///
    /// Starts the video stream
    ///
    /// The returned object manages the data stream, when it is dropped
    /// the video stop signal is sent to the camera
    ///
    /// To pull frames from the camera's buffer use `recv_data` on the returned object
    ///
    /// The buffer_size represents number of compete messages so 1 would be one complete message
    /// which may be a single audio frame or a whole video key frame. If 0 a default of 100 is used
    ///
    /// A value of scrict=true will mean that the stream will error if the underlying stream is not
    /// as expected
    pub async fn start_video(
        &self,
        stream: StreamKind,
        mut buffer_size: usize,
        strict: bool,
    ) -> Result<StreamData> {
        if let Err(e) = self.has_ability_rw("preview").await {
            if self.has_ability_ro("streamTable").await.is_err() {
                return Err(e);
            }
        }

        let connection = self.get_connection();
        let msg_num = self.new_message_num();

        let abort_handle = CancellationToken::new();
        let abort_handle_thread = abort_handle.clone();

        if buffer_size == 0 {
            buffer_size = 100;
        }
        let (tx, rx) = channel(buffer_size);
        let channel_id = self.channel_id;

        let handle = task::spawn(async move {
            let mut sub_video = connection.subscribe(MSG_ID_VIDEO, msg_num).await?;

            // On an E1 and swann cameras:
            //  - mainStream always has a value of 0
            //  - subStream always has a value of 1
            //  - There is no externStram
            // On a B800:
            //  - mainStream is 0
            //  - subStream is 0
            //  - externStream is 0
            let stream_code = match stream {
                StreamKind::Main => 0,
                StreamKind::Sub => 1,
                StreamKind::Extern => 0,
            };

            // Theses are the numbers used with the offical client
            // On an E1 and swann cameras:
            //  - mainStream always has a value of 0
            //  - subStream always has a value of 1
            //  - There is no externStram
            // On a B800:
            //  - mainStream is 0
            //  - subStream is 256
            //  - externStram is 1024
            let handle = match stream {
                StreamKind::Main => 0,
                StreamKind::Sub => 256,
                StreamKind::Extern => 1024,
            };

            let stream_name = match stream {
                StreamKind::Main => "mainStream",
                StreamKind::Sub => "subStream",
                StreamKind::Extern => "externStream",
            }
            .to_string();

            let start_video = Bc::new_from_xml(
                BcMeta {
                    msg_id: MSG_ID_VIDEO,
                    channel_id,
                    msg_num,
                    stream_type: stream_code,
                    response_code: 0,
                    class: 0x6414, // IDK why
                },
                BcXml {
                    preview: Some(Preview {
                        version: xml_ver(),
                        channel_id,
                        handle,
                        stream_type: Some(stream_name),
                    }),
                    ..Default::default()
                },
            );

            sub_video.send(start_video).await?;

            let msg = sub_video.recv().await?;
            if let BcMeta {
                response_code: 200, ..
            } = msg.meta
            {
            } else {
                return Err(Error::UnintelligibleReply {
                    reply: std::sync::Arc::new(Box::new(msg)),
                    why: "The camera did not accept the stream start command.",
                });
            }

            {
                let mut media_sub = sub_video.bcmedia_stream(strict);

                tokio::select! {
                    _ = abort_handle_thread.cancelled() => {},
                    _ = async {
                        while let Some(bc_media) = media_sub.next().await {
                            // debug!("Stream: Got");
                            // We now have a complete interesting packet. Send it to on the callback
                            // debug!("Stream: Send");
                            if tx.send(bc_media).await.is_err() {
                                // debug!("Stream: Dropped");
                                break; // Connection dropped
                            }
                            // debug!("Stream: Sent");
                        }
                    } => {}
                }
            }

            // debug!("Stream: Stopping");
            let stop_video = Bc::new_from_xml(
                BcMeta {
                    msg_id: MSG_ID_VIDEO_STOP,
                    channel_id,
                    msg_num,
                    stream_type: stream_code,
                    response_code: 0,
                    class: 0x6414, // IDK why
                },
                BcXml {
                    preview: Some(Preview {
                        version: xml_ver(),
                        channel_id,
                        handle,
                        stream_type: None,
                    }),
                    ..Default::default()
                },
            );
            // debug!("Stream: Send Stop");
            let mut sub_stop = connection.subscribe(MSG_ID_VIDEO_STOP, msg_num).await?;
            sub_stop.send(stop_video).await?;
            // debug!("Stream: Sent Stop");

            tokio::select! {
                v = async {
                    loop {
                        let msg = sub_stop.recv().await?;
                        if let BcMeta {
                            response_code: 200,
                            msg_id: MSG_ID_VIDEO_STOP,
                            ..
                        } = msg.meta {
                            return Ok(());
                        }
                        else if let BcMeta {
                            msg_id: MSG_ID_VIDEO_STOP,
                            ..
                        }   = msg.meta {
                            return Err(Error::CameraServiceUnavaliable);
                        }
                    }
                } => v,
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {Ok(())},
            }?;

            Ok(())
        });

        Ok(StreamData {
            handle: Some(handle),
            rx,
            abort_handle,
        })
    }

    /// Stop a camera from sending more stream data.
    pub async fn stop_video(&self, stream: StreamKind) -> Result<()> {
        if let Err(e) = self.has_ability_rw("preview").await {
            if self.has_ability_ro("streamTable").await.is_err() {
                return Err(e);
            }
        }
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_video = connection.subscribe(MSG_ID_VIDEO_STOP, msg_num).await?;

        // On an E1 and swann cameras:
        //  - mainStream always has a value of 0
        //  - subStream always has a value of 1
        //  - There is no externStram
        // On a B800:
        //  - mainStream is 0
        //  - subStream is 0
        //  - externStream is 0
        let stream_code = match stream {
            StreamKind::Main => 0,
            StreamKind::Sub => 1,
            StreamKind::Extern => 0,
        };

        // Theses are the numbers used with the offical client
        // On an E1 and swann cameras:
        //  - mainStream always has a value of 0
        //  - subStream always has a value of 1
        //  - There is no externStram
        // On a B800:
        //  - mainStream is 0
        //  - subStream is 256
        //  - externStram is 1024
        let handle = match stream {
            StreamKind::Main => 0,
            StreamKind::Sub => 256,
            StreamKind::Extern => 1024,
        };

        let stop_video = Bc::new_from_xml(
            BcMeta {
                msg_id: MSG_ID_VIDEO_STOP,
                channel_id: self.channel_id,
                msg_num,
                stream_type: stream_code,
                response_code: 0,
                class: 0x6414, // IDK why
            },
            BcXml {
                preview: Some(Preview {
                    version: xml_ver(),
                    channel_id: self.channel_id,
                    handle,
                    stream_type: None,
                }),
                ..Default::default()
            },
        );

        sub_video.send(stop_video).await?;

        let reply = sub_video.recv().await?;
        if reply.meta.response_code != 200 {
            return Err(super::Error::CameraServiceUnavaliable);
        }

        Ok(())
    }
}

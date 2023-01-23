use super::{BcCamera, BinarySubscriber, Error, Result, RX_TIMEOUT};
use crate::{
    bc::{model::*, xml::*},
    bcmedia::model::*,
};
use crossbeam_channel::{bounded, unbounded, Receiver, TryRecvError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};

/// The stream names supported by BC
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Stream {
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

/// A handle on currently streaming data
///
/// The data can be pulled using `get_data` which returns raw BcMedia packets
///
/// When this object is dropped the streaming is stopped
pub struct StreamData<'a> {
    camera: &'a BcCamera,
    handle: Option<JoinHandle<Result<()>>>,
    rx: Receiver<Result<BcMedia>>,
    abort_handle: Arc<AtomicBool>,
    stream: Stream,
}

impl<'a> StreamData<'a> {
    /// Pull data from the camera's buffer
    /// This returns raw BcMedia packets
    pub fn get_data(&self) -> Result<Vec<Result<BcMedia>>> {
        let mut results: Vec<_> = vec![];
        loop {
            match self.rx.try_recv() {
                Ok(data) => results.push(data),
                Err(TryRecvError::Empty) => break,
                Err(e) => return Err(Error::from(e)),
            }
        }
        Ok(results)
    }
}

impl<'a> Drop for StreamData<'a> {
    fn drop(&mut self) {
        self.abort_handle.store(true, Ordering::Relaxed);
        self.handle.take().map(|h| h.join());
        let _ = self.camera.stop_video(self.stream);
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
    /// The buffer represents number of compete messages so 1 would be one complete message
    /// which may be a single audio frame or a whole video key frame
    /// A value of 0 means unlimited buffer size
    ///
    pub fn start_video(&self, stream: Stream, buffer_size: usize) -> Result<StreamData<'_>> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let channel_id = self.channel_id;

        let abort_handle = Arc::new(AtomicBool::new(false));
        let (tx, rx) = match buffer_size {
            0 => unbounded(),
            n => bounded(n),
        };

        let abort_handle_thread = abort_handle.clone();
        let handle = thread::spawn(move || {
            let sub_video = connection.subscribe(msg_num)?;

            // On an E1 and swann cameras:
            //  - mainStream always has a value of 0
            //  - subStream always has a value of 1
            //  - There is no externStram
            // On a B800:
            //  - mainStream is 0
            //  - subStream is 0
            //  - externStream is 0
            let stream_code = match stream {
                Stream::Main => 0,
                Stream::Sub => 1,
                Stream::Extern => 0,
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
                Stream::Main => 0,
                Stream::Sub => 256,
                Stream::Extern => 1024,
            };

            let stream_name = match stream {
                Stream::Main => "mainStream",
                Stream::Sub => "subStream",
                Stream::Extern => "externStream",
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

            sub_video.send(start_video)?;

            let msg = sub_video.rx.recv_timeout(RX_TIMEOUT)?;
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

            let mut media_sub = BinarySubscriber::from_bc_sub(&sub_video);

            while !abort_handle_thread.load(Ordering::Relaxed) {
                let bc_media = BcMedia::deserialize(&mut media_sub).map_err(Error::from);
                // We now have a complete interesting packet. Send it to on the callback
                if tx.send(bc_media).is_err() {
                    break; // Connection dropped
                }
            }
            Ok(())
        });

        Ok(StreamData {
            camera: self,
            handle: Some(handle),
            rx,
            stream,
            abort_handle,
        })
    }

    /// Stop a camera from sending more stream data.
    pub fn stop_video(&self, stream: Stream) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let sub_video = connection.subscribe(msg_num)?;

        // On an E1 and swann cameras:
        //  - mainStream always has a value of 0
        //  - subStream always has a value of 1
        //  - There is no externStram
        // On a B800:
        //  - mainStream is 0
        //  - subStream is 0
        //  - externStream is 0
        let stream_code = match stream {
            Stream::Main => 0,
            Stream::Sub => 1,
            Stream::Extern => 0,
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
            Stream::Main => 0,
            Stream::Sub => 256,
            Stream::Extern => 1024,
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

        sub_video.send(stop_video)?;

        let reply = sub_video.rx.recv_timeout(crate::RX_TIMEOUT)?;
        if reply.meta.response_code != 200 {
            return Err(super::Error::CameraServiceUnavaliable);
        }

        Ok(())
    }
}

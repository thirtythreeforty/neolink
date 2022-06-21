use super::{BcCamera, BinarySubscriber, Result};
use crate::{
    bc::{model::*, xml::*},
    bcmedia::model::*,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Convience type for the error raised by the [StreamOutput] trait
pub type StreamOutputError = Result<()>;

/// The method [`BcCamera::start_video()`] requires a structure with this trait to pass the
/// audio and video data back to
pub trait StreamOutput {
    /// This is the callback raised a complete media packet is received
    fn write(&mut self, media: BcMedia) -> StreamOutputError;
}

/// The stream names supported by BC
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

impl BcCamera {
    ///
    /// Starts the video stream
    ///
    /// # Parameters
    ///
    /// * `data_outs` - This should be a struct that implements the [`StreamOutput`] trait
    ///
    /// * `stream_name` - This is a [`Stream`] that controls the stream to request from the camera.
    ///                   Selecting [`Stream::Main`] will select the HD stream.
    ///                   Selecting [`Stream::Sub`] will select the SD stream.
    ///                   Selecting [`Stream::Extern`] will select the medium quality stream (only on some camera)
    ///
    /// # Returns
    ///
    /// This will block forever or return an error when the camera connection is dropped
    ///
    pub fn start_video<Outputs>(
        &self,
        data_outs: &mut Outputs,
        stream: Stream,
        abort_handle: Arc<AtomicBool>,
    ) -> Result<()>
    where
        Outputs: StreamOutput,
    {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to start video");
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

        let stream_name = match stream {
            Stream::Main => "mainStream",
            Stream::Sub => "subStream",
            Stream::Extern => "externStream",
        }
        .to_string();

        let start_video = Bc::new_from_xml(
            BcMeta {
                msg_id: MSG_ID_VIDEO,
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
                    stream_type: stream_name,
                }),
                ..Default::default()
            },
        );

        sub_video.send(start_video)?;

        let mut media_sub = BinarySubscriber::from_bc_sub(&sub_video);

        while !abort_handle.load(Ordering::Relaxed) {
            let bc_media = BcMedia::deserialize(&mut media_sub)?;
            // We now have a complete interesting packet. Send it to on the callback
            data_outs.write(bc_media)?;
        }

        // Aborted
        Ok(())
    }

    /// Stop a camera from sending more stream data.
    pub fn stop_video(&self, stream: Stream) -> Result<()> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to stop video");
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

        let stream_name = match stream {
            Stream::Main => "mainStream",
            Stream::Sub => "subStream",
            Stream::Extern => "externStream",
        }
        .to_string();

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
                    stream_type: stream_name,
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

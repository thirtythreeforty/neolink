use super::{BcCamera, BinarySubscriber, Result};
use crate::{
    bc::{model::*, xml::*},
    Never,
};

/// Convience type for the error raised by the [StreamOutput] trait
pub type StreamOutputError = Result<()>;

/// The method [`BcCamera::start_video()`] requires a structure with this trait to pass the
/// audio and video data back to
pub trait StreamOutput {
    /// This is the callback raised a complete media packet is received
    fn write(&mut self, media: BcMedia) -> StreamOutputError;
}

impl BcCamera {
    ///
    /// Starts the video stream
    ///
    /// # Parameters
    ///
    /// * `data_outs` - This should be a struct that implements the [`StreamOutput`] trait
    ///
    /// * `stream_name` - The name of the stream either `"mainStream"` for HD or `"subStream"` for SD
    ///
    /// # Returns
    ///
    /// This will block forever or return an error when the camera connection is dropped
    ///
    pub fn start_video<Outputs>(&self, data_outs: &mut Outputs, stream_name: &str) -> Result<Never>
    where
        Outputs: StreamOutput,
    {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to start video");
        let sub_video = connection.subscribe(MSG_ID_VIDEO)?;

        let stream_num = match stream_name {
            "mainStream" => 0,
            "subStream" => 1,
            _ => 0,
        };

        let start_video = Bc::new_from_xml(
            BcMeta {
                msg_id: MSG_ID_VIDEO,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                stream_type: stream_num,
                response_code: 0,
                class: 0x6414, // IDK why
            },
            BcXml {
                preview: Some(Preview {
                    version: xml_ver(),
                    channel_id: self.channel_id,
                    handle: 0,
                    stream_type: stream_name.to_string(),
                }),
                ..Default::default()
            },
        );

        sub_video.send(start_video)?;

        let mut media_sub = BinarySubscriber::from_bc_sub(&sub_video);

        loop {
            let bc_media = BcMedia::deserialize(&mut media_sub).map_err(|e| {
                log::error!("{:?}", e);
                e
            })?;
            // We now have a complete interesting packet. Send it to on the callback
            data_outs.write(bc_media)?;
        }
    }
}

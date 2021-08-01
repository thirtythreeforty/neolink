use super::{
    media_packet::{MediaDataKind, MediaDataSubscriber},
    BcCamera, Result,
};
use crate::{
    bc::{model::*, xml::*},
    Never,
};

/// The stream from the camera will be using one of these formats
#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub enum StreamFormat {
    /// H264 (AVC) video format
    H264,
    /// H265 (HEVC) video format
    H265,
    /// AAC audio
    AAC,
    /// ADPCM in DVI-4 format
    ADPCM,
}

/// Convience type for the error raised by the StreamOutput trait
pub type StreamOutputError = Result<()>;

/// The method `start_stream` requires a structure with this trait to pass the
/// audio and video data back to
pub trait StreamOutput {
    /// This is the callback raised when audio data is received
    fn write_audio(&mut self, data: &[u8], format: StreamFormat) -> StreamOutputError;
    /// This is the callback raised when video data is received
    fn write_video(&mut self, data: &[u8], format: StreamFormat) -> StreamOutputError;
}

impl BcCamera {
    ///
    /// Starts the video stream
    ///
    /// # Parameters
    ///
    /// * `data_outs` - This should be a struct that implements the `StreamOutput` trait
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

        let mut media_sub = MediaDataSubscriber::from_bc_sub(&sub_video);

        loop {
            let binary_data = media_sub.next_media_packet()?;
            // We now have a complete interesting packet. Send it to gst.
            // Process the packet
            match (binary_data.kind(), binary_data.media_format()) {
                (
                    MediaDataKind::VideoDataIframe | MediaDataKind::VideoDataPframe,
                    Some(media_format),
                ) => {
                    data_outs.write_video(binary_data.body(), media_format)?;
                }
                (MediaDataKind::AudioDataAac, Some(media_format)) => {
                    data_outs.write_audio(binary_data.body(), media_format)?;
                }
                (MediaDataKind::AudioDataAdpcm, Some(media_format)) => {
                    data_outs.write_audio(binary_data.body(), media_format)?;
                }
                _ => {}
            };
        }
    }
}

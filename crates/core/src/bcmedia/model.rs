/// Video streams encapsulate a stream of BcMedia
#[derive(Debug, Clone)]
pub enum BcMedia {
    /// Holds info on the stream
    InfoV1(BcMediaInfoV1),
    /// Holds info on the stream
    InfoV2(BcMediaInfoV2),
    /// Holds an IFrame either H264 or H265
    Iframe(BcMediaIframe),
    /// Holds a PFrame either H264 or H265
    Pframe(BcMediaPframe),
    /// Holds AAC audio
    Aac(BcMediaAac),
    /// Holds ADPCM audio
    Adpcm(BcMediaAdpcm),
}
//
pub(super) const MAGIC_HEADER_BCMEDIA_INFO_V1: u32 = 0x31303031;

/// The start of a BcMedia stream contains this message
/// which describes the data to follow
#[derive(Debug, Clone)]
pub struct BcMediaInfoV1 {
    // This is the size of the header so it's actually a fixed value
    // The other messages have body size here so maybe that's why
    // it's included
    // pub header_size: u32,
    /// Width of the video
    pub video_width: u32,
    /// Height of the video
    pub video_height: u32,
    // pub unknown: u8,
    /// Frames per second. On older cameras this seems to be an index of the FPS on a lookup table
    pub fps: u8,
    /// Start year of the stream
    pub start_year: u8,
    /// Start month of the stream
    pub start_month: u8,
    /// Start day of the stream
    pub start_day: u8,
    /// Start hour of the stream
    pub start_hour: u8,
    /// Start minute of the stream
    pub start_min: u8,
    /// Start seconds of the stream
    pub start_seconds: u8,
    /// End year of the video probably only useful for the recorded files on the SD card
    pub end_year: u8,
    /// End month of the video probably only useful for the recorded files on the SD card
    pub end_month: u8,
    /// End day of the video probably only useful for the recorded files on the SD card
    pub end_day: u8,
    /// End hour of the video probably only useful for the recorded files on the SD card
    pub end_hour: u8,
    /// End min of the video probably only useful for the recorded files on the SD card
    pub end_min: u8,
    /// End seconds of the video probably only useful for the recorded files on the SD card
    pub end_seconds: u8,
    // unknown: u16
}
//
pub(super) const MAGIC_HEADER_BCMEDIA_INFO_V2: u32 = 0x32303031;

/// The start of a BcMedia stream contains this message
/// which describes the data to follow
#[derive(Debug, Clone)]
pub struct BcMediaInfoV2 {
    // This is the size of the header so it's actually a fixed value
    // The other messages have body size here so maybe that's why
    // it's included
    // pub header_size: u32,
    /// Width of the video
    pub video_width: u32,
    /// Height of the video
    pub video_height: u32,
    // pub unknown: u8,
    /// Frames per second. On older cameras this seems to be an index of the FPS on a lookup table
    pub fps: u8,
    /// Start year of the stream
    pub start_year: u8,
    /// Start month of the stream
    pub start_month: u8,
    /// Start day of the stream
    pub start_day: u8,
    /// Start hour of the stream
    pub start_hour: u8,
    /// Start minute of the stream
    pub start_min: u8,
    /// Start seconds of the stream
    pub start_seconds: u8,
    /// End year of the video probably only useful for the recorded files on the SD card
    pub end_year: u8,
    /// End month of the video probably only useful for the recorded files on the SD card
    pub end_month: u8,
    /// End day of the video probably only useful for the recorded files on the SD card
    pub end_day: u8,
    /// End hour of the video probably only useful for the recorded files on the SD card
    pub end_hour: u8,
    /// End min of the video probably only useful for the recorded files on the SD card
    pub end_min: u8,
    /// End seconds of the video probably only useful for the recorded files on the SD card
    pub end_seconds: u8,
    // unknown: u16
}

// IFrame magics include the channel number in them
pub(super) const MAGIC_HEADER_BCMEDIA_IFRAME: u32 = 0x63643030;
pub(super) const MAGIC_HEADER_BCMEDIA_IFRAME_LAST: u32 = 0x63643039;

/// Video Types for I/PFrame
#[derive(Debug, Clone)]
pub enum VideoType {
    /// H264 video data
    H264,
    /// H265 video data
    H265,
}

/// This is a BcMedia video IFrame.
#[derive(Clone)]
pub struct BcMediaIframe {
    /// "H264", or "H265"
    pub video_type: VideoType,
    // Size of payload after header in bytes
    // pub payload_size: u32,
    // unknown: u32, // NVR channel count? Known values 1-00/08 2-00 3-00 4-00
    /// Timestamp in microseconds
    pub microseconds: u32,
    // unknown: u32, // Known values 1-00/23/5A 2-00 3-00 4-00
    /// POSIX time (seconds since 00:00:00 Jan 1 1970)
    pub time: Option<u32>,
    //unknown: u32, // Known values 1-00/06/29 2-00/01 3-00/C3 4-00
    /// Raw IFrame data
    pub data: Vec<u8>,
}

impl std::fmt::Debug for BcMediaIframe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entry(&"video_type", &self.video_type)
            // .entry(&"payload_size", &self.payload_size)
            .entry(&"microseconds", &self.microseconds)
            .entry(&"time", &self.time)
            .entry(
                &"data[0..10]",
                &self.data[0..std::cmp::min(20, self.data.len())].to_vec(),
            )
            .entry(
                &"data[-10..-1]",
                &self.data[std::cmp::max(0, self.data.len() - 20)..self.data.len()].to_vec(),
            )
            .entry(&"data.len()", &self.data.len())
            .finish()
    }
}

// PFrame magics include the channel number in them
pub(super) const MAGIC_HEADER_BCMEDIA_PFRAME: u32 = 0x63643130;
pub(super) const MAGIC_HEADER_BCMEDIA_PFRAME_LAST: u32 = 0x63643139;

/// This is a BcMedia video PFrame.
#[derive(Clone)]
pub struct BcMediaPframe {
    /// "H264", or "H265"
    pub video_type: VideoType,
    // Size of payload after header in bytes
    // pub payload_size: u32,
    // unknown: u32, // NVR channel count? Known values 1-00/08 2-00 3-00 4-00
    /// Timestamp in microseconds
    pub microseconds: u32,
    // unknown: u32, // Known values 1-00/23/5A 2-00 3-00 4-00
    /// Raw PFrame data
    pub data: Vec<u8>,
}

impl std::fmt::Debug for BcMediaPframe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entry(&"video_type", &self.video_type)
            // .entry(&"payload_size", &self.payload_size)
            .entry(&"microseconds", &self.microseconds)
            .entry(
                &"data[0..20]",
                &self.data[0..std::cmp::min(20, self.data.len())].to_vec(),
            )
            .entry(
                &"data[-20..-1]",
                &self.data[std::cmp::max(0, self.data.len() - 20)..self.data.len()].to_vec(),
            )
            .entry(&"data.len()", &self.data.len())
            .finish()
    }
}

pub(super) const MAGIC_HEADER_BCMEDIA_AAC: u32 = 0x62773530;

/// This contains BcMedia audio data in AAC format
#[derive(Debug, Clone)]
pub struct BcMediaAac {
    // Size of payload after header in bytes
    // pub payload_size: u16,
    // Size of payload after header in bytes exactly the same as before
    // pub payload_size_b: u16,
    /// Raw AAC data
    pub data: Vec<u8>,
}

pub(super) const MAGIC_HEADER_BCMEDIA_ADPCM: u32 = 0x62773130;

pub(super) const MAGIC_HEADER_BCMEDIA_ADPCM_DATA: u16 = 0x0100;

/// This contains BcMedia audio data in ADPCM format
#[derive(Debug, Clone)]
pub struct BcMediaAdpcm {
    // Size of payload after header in bytes
    // pub payload_size: u16,
    // Size of payload after header in bytes exactly the same as before
    // pub payload_size_b: u16,
    // more_magic: MAGIC_HEADER_BCMEDIA_ADPCM_DATA
    // Adpcm sample_block_size in bytes
    //
    // These bytes (and the MAGIC_HEADER_BCMEDIA_ADPCM_DATA) are included as
    // part of the payload_size. It may be more prudent to sealise them to
    // another structure.
    // pub sample_block_size: u16,
    /// The raw adpcm data in DVI-4 layout.
    ///
    /// One `data` should contain 4 bytes of the adpcm predictor state then one block
    /// of adpcm samples
    ///
    /// To calculate the block-align size simply remove 4 from the `len()`
    pub data: Vec<u8>,
}

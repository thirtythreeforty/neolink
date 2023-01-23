use super::model::*;
use cookie_factory::bytes::*;
use cookie_factory::sequence::tuple;
use cookie_factory::{combinator::*, gen};
use cookie_factory::{GenError, SerializeFn};
use err_derive::Error;
use log::error;
use std::io::Write;

// PAD_SIZE: Media packets use 8 byte padding
const PAD_SIZE: u32 = 8;

/// The error types used during serialisation
#[derive(Debug, Error, Clone)]
pub enum Error {
    /// A Cookie Factor  GenError
    #[error(display = "Cookie GenError")]
    GenError(#[error(source)] std::sync::Arc<GenError>),
}

impl From<GenError> for Error {
    fn from(k: GenError) -> Self {
        Error::GenError(std::sync::Arc::new(k))
    }
}

impl BcMedia {
    pub(crate) fn serialize<W: Write>(&self, buf: W) -> Result<W, Error> {
        let (buf, _) = match &self {
            BcMedia::InfoV1(payload) => gen(bcmedia_info_v1(payload), buf)?,
            BcMedia::InfoV2(payload) => gen(bcmedia_info_v2(payload), buf)?,
            BcMedia::Iframe(payload) => {
                let pad_size = match payload.data.len() as u32 % PAD_SIZE {
                    0 => 0,
                    n => PAD_SIZE - n,
                };
                gen(
                    tuple((
                        bcmedia_iframe(payload),
                        slice(&payload.data),
                        slice(&vec![0; pad_size as usize]),
                    )),
                    buf,
                )?
            }
            BcMedia::Pframe(payload) => {
                let pad_size = match payload.data.len() as u32 % PAD_SIZE {
                    0 => 0,
                    n => PAD_SIZE - n,
                };
                gen(
                    tuple((
                        bcmedia_pframe(payload),
                        slice(&payload.data),
                        slice(&vec![0; pad_size as usize]),
                    )),
                    buf,
                )?
            }
            BcMedia::Aac(payload) => {
                let pad_size = match payload.data.len() as u32 % PAD_SIZE {
                    0 => 0,
                    n => PAD_SIZE - n,
                };
                gen(
                    tuple((
                        bcmedia_aac(payload),
                        slice(&payload.data),
                        slice(&vec![0; pad_size as usize]),
                    )),
                    buf,
                )?
            }
            BcMedia::Adpcm(payload) => {
                let pad_size = match payload.data.len() as u32 % PAD_SIZE {
                    0 => 0,
                    n => PAD_SIZE - n,
                };
                gen(
                    tuple((
                        bcmedia_adpcm(payload),
                        slice(&payload.data),
                        slice(&vec![0; pad_size as usize]),
                    )),
                    buf,
                )?
            }
        };

        Ok(buf)
    }
}

fn bcmedia_info_v1<W: Write>(payload: &BcMediaInfoV1) -> impl SerializeFn<W> {
    tuple((
        le_u32(MAGIC_HEADER_BCMEDIA_INFO_V1),
        le_u32(32),
        le_u32(payload.video_width),
        le_u32(payload.video_height),
        le_u8(0), // unknown. Known values 00/01
        le_u8(payload.fps),
        le_u8(payload.start_year),
        le_u8(payload.start_month),
        le_u8(payload.start_day),
        le_u8(payload.start_hour),
        le_u8(payload.start_min),
        le_u8(payload.start_seconds),
        le_u8(payload.end_year),
        le_u8(payload.end_month),
        le_u8(payload.end_day),
        le_u8(payload.end_hour),
        le_u8(payload.end_min),
        le_u8(payload.end_seconds),
        le_u8(0),
        le_u8(0),
    ))
}

fn bcmedia_info_v2<W: Write>(payload: &BcMediaInfoV2) -> impl SerializeFn<W> {
    tuple((
        le_u32(MAGIC_HEADER_BCMEDIA_INFO_V2),
        le_u32(32),
        le_u32(payload.video_width),
        le_u32(payload.video_height),
        le_u8(0), // unknown. Known values 00/01
        le_u8(payload.fps),
        le_u8(payload.start_year),
        le_u8(payload.start_month),
        le_u8(payload.start_day),
        le_u8(payload.start_hour),
        le_u8(payload.start_min),
        le_u8(payload.start_seconds),
        le_u8(payload.end_year),
        le_u8(payload.end_month),
        le_u8(payload.end_day),
        le_u8(payload.end_hour),
        le_u8(payload.end_min),
        le_u8(payload.end_seconds),
        le_u8(0),
        le_u8(0),
    ))
}

fn bcmedia_iframe<W: Write>(payload: &BcMediaIframe) -> impl SerializeFn<W> {
    // Cookie String needs a static lifetime
    let vid_string = match payload.video_type {
        VideoType::H264 => "H264",
        VideoType::H265 => "H265",
    };
    let (extra_header, extra_header_size) = if let Some(payload_time) = payload.time {
        let extra_header = slice(
            gen(tuple((le_u32(payload_time), le_u32(0))), vec![])
                .unwrap()
                .0,
        );
        let extra_header_size = 8;
        (extra_header, extra_header_size)
    } else {
        let extra_header = slice(vec![]);
        let extra_header_size = 0;
        (extra_header, extra_header_size)
    };
    tuple((
        le_u32(MAGIC_HEADER_BCMEDIA_IFRAME),
        string(vid_string),
        le_u32(payload.data.len() as u32),
        le_u32(extra_header_size), //  unknown. NVR channel count? Known values 1-00/08 2-00 3-00 4-00
        le_u32(payload.microseconds),
        le_u32(0), // unknown. Known values 1-00/23/5A 2-00 3-00 4-00
        extra_header,
    ))
}

fn bcmedia_pframe<W: Write>(payload: &BcMediaPframe) -> impl SerializeFn<W> {
    // Cookie String needs a static lifetime
    let vid_string = match payload.video_type {
        VideoType::H264 => "H264",
        VideoType::H265 => "H265",
    };
    tuple((
        le_u32(MAGIC_HEADER_BCMEDIA_PFRAME),
        string(vid_string),
        le_u32(payload.data.len() as u32),
        le_u32(0), //  unknown. NVR channel count? Known values 1-00/08 2-00 3-00 4-00
        le_u32(payload.microseconds),
        le_u32(0), // unknown. Known values 1-00/23/5A 2-00 3-00 4-00
    ))
}

fn bcmedia_aac<W: Write>(payload: &BcMediaAac) -> impl SerializeFn<W> {
    tuple((
        le_u32(MAGIC_HEADER_BCMEDIA_AAC),
        le_u16(payload.data.len() as u16),
        le_u16(payload.data.len() as u16),
    ))
}

fn bcmedia_adpcm<W: Write>(payload: &BcMediaAdpcm) -> impl SerializeFn<W> {
    tuple((
        le_u32(MAGIC_HEADER_BCMEDIA_ADPCM),
        le_u16((payload.data.len() + 4) as u16), // Payload + 2 byte magic + 2byte block size
        le_u16((payload.data.len() + 4) as u16), // Payload + 2 byte magic + 2byte block size
        le_u16(MAGIC_HEADER_BCMEDIA_ADPCM_DATA), // magic
        le_u16(((payload.data.len() - 4) / 2) as u16), // Block size without the header halved
    ))
}

use super::model::BcMediaIframe;
use super::model::*;
use crate::Error;
use bytes::{Buf, BytesMut};
use nom::{combinator::*, error::context, number::streaming::*, take};

type IResult<I, O, E = nom::error::VerboseError<I>> = Result<(I, O), nom::Err<E>>;

// PAD_SIZE: Media packets use 8 byte padding
const PAD_SIZE: u32 = 8;

impl BcMedia {
    pub(crate) fn deserialize(buf: &mut BytesMut) -> Result<BcMedia, Error> {
        const TYPICAL_HEADER: usize = 25;
        let (result, len) = match consumed(bcmedia)(buf) {
            Ok((_, (parsed_buff, result))) => Ok((result, parsed_buff.len())),
            Err(e) => Err(e),
        }?;
        buf.advance(len);
        buf.reserve(len + TYPICAL_HEADER); // Preallocate for future buffer calls
        Ok(result)
    }
}

fn bcmedia(buf: &[u8]) -> IResult<&[u8], BcMedia> {
    let (buf, magic) = context(
        "Failed to match any known bcmedia",
        verify(le_u32, |x| {
            matches!(
                *x,
                MAGIC_HEADER_BCMEDIA_INFO_V1
                    | MAGIC_HEADER_BCMEDIA_INFO_V2
                    | MAGIC_HEADER_BCMEDIA_IFRAME..=MAGIC_HEADER_BCMEDIA_IFRAME_LAST
                    | MAGIC_HEADER_BCMEDIA_PFRAME..=MAGIC_HEADER_BCMEDIA_PFRAME_LAST
                    | MAGIC_HEADER_BCMEDIA_AAC
                    | MAGIC_HEADER_BCMEDIA_ADPCM
            )
        }),
    )(buf)?;

    match magic {
        MAGIC_HEADER_BCMEDIA_INFO_V1 => {
            let (buf, payload) = bcmedia_info_v1(buf)?;
            Ok((buf, BcMedia::InfoV1(payload)))
        }
        MAGIC_HEADER_BCMEDIA_INFO_V2 => {
            let (buf, payload) = bcmedia_info_v2(buf)?;
            Ok((buf, BcMedia::InfoV2(payload)))
        }
        MAGIC_HEADER_BCMEDIA_IFRAME..=MAGIC_HEADER_BCMEDIA_IFRAME_LAST => {
            let (buf, payload) = bcmedia_iframe(buf)?;
            Ok((buf, BcMedia::Iframe(payload)))
        }
        MAGIC_HEADER_BCMEDIA_PFRAME..=MAGIC_HEADER_BCMEDIA_PFRAME_LAST => {
            let (buf, payload) = bcmedia_pframe(buf)?;
            Ok((buf, BcMedia::Pframe(payload)))
        }
        MAGIC_HEADER_BCMEDIA_AAC => {
            let (buf, payload) = bcmedia_aac(buf)?;
            Ok((buf, BcMedia::Aac(payload)))
        }
        MAGIC_HEADER_BCMEDIA_ADPCM => {
            let (buf, payload) = bcmedia_adpcm(buf)?;
            Ok((buf, BcMedia::Adpcm(payload)))
        }
        _ => unreachable!(),
    }
}

fn bcmedia_info_v1(buf: &[u8]) -> IResult<&[u8], BcMediaInfoV1> {
    let (buf, _header_size) = context(
        "Header size mismatch in BCMedia InfoV1",
        verify(le_u32, |x| *x == 32),
    )(buf)?;
    let (buf, video_width) = le_u32(buf)?;
    let (buf, video_height) = le_u32(buf)?;
    let (buf, _unknown) = le_u8(buf)?;
    let (buf, fps) = le_u8(buf)?;
    let (buf, start_year) = le_u8(buf)?;
    let (buf, start_month) = le_u8(buf)?;
    let (buf, start_day) = le_u8(buf)?;
    let (buf, start_hour) = le_u8(buf)?;
    let (buf, start_min) = le_u8(buf)?;
    let (buf, start_seconds) = le_u8(buf)?;
    let (buf, end_year) = le_u8(buf)?;
    let (buf, end_month) = le_u8(buf)?;
    let (buf, end_day) = le_u8(buf)?;
    let (buf, end_hour) = le_u8(buf)?;
    let (buf, end_min) = le_u8(buf)?;
    let (buf, end_seconds) = le_u8(buf)?;
    let (buf, _unknown_b) = le_u16(buf)?;

    Ok((
        buf,
        BcMediaInfoV1 {
            // header_size,
            video_width,
            video_height,
            fps,
            start_year,
            start_month,
            start_day,
            start_hour,
            start_min,
            start_seconds,
            end_year,
            end_month,
            end_day,
            end_hour,
            end_min,
            end_seconds,
        },
    ))
}

fn bcmedia_info_v2(buf: &[u8]) -> IResult<&[u8], BcMediaInfoV2> {
    let (buf, _header_size) = context(
        "Failed to match headersize in BCMedia Info V2",
        verify(le_u32, |x| *x == 32),
    )(buf)?;
    let (buf, video_width) = le_u32(buf)?;
    let (buf, video_height) = le_u32(buf)?;
    let (buf, _unknown) = le_u8(buf)?;
    let (buf, fps) = le_u8(buf)?;
    let (buf, start_year) = le_u8(buf)?;
    let (buf, start_month) = le_u8(buf)?;
    let (buf, start_day) = le_u8(buf)?;
    let (buf, start_hour) = le_u8(buf)?;
    let (buf, start_min) = le_u8(buf)?;
    let (buf, start_seconds) = le_u8(buf)?;
    let (buf, end_year) = le_u8(buf)?;
    let (buf, end_month) = le_u8(buf)?;
    let (buf, end_day) = le_u8(buf)?;
    let (buf, end_hour) = le_u8(buf)?;
    let (buf, end_min) = le_u8(buf)?;
    let (buf, end_seconds) = le_u8(buf)?;
    let (buf, _unknown_b) = le_u16(buf)?;

    Ok((
        buf,
        BcMediaInfoV2 {
            // header_size,
            video_width,
            video_height,
            fps,
            start_year,
            start_month,
            start_day,
            start_hour,
            start_min,
            start_seconds,
            end_year,
            end_month,
            end_day,
            end_hour,
            end_min,
            end_seconds,
        },
    ))
}

fn take4(buf: &[u8]) -> IResult<&[u8], &str> {
    map_res(nom::bytes::streaming::take(4usize), |r| {
        std::str::from_utf8(r)
    })(buf)
}

fn bcmedia_iframe(buf: &[u8]) -> IResult<&[u8], BcMediaIframe> {
    let (buf, video_type_str) = context(
        "Video Type is unrecognised in IFrame",
        verify(take4, |x| matches!(x, "H264" | "H265")),
    )(buf)?;
    let (buf, payload_size) = le_u32(buf)?;
    let (buf, additional_header_size) = le_u32(buf)?;
    let (buf, microseconds) = le_u32(buf)?;
    let (buf, _unknown_b) = le_u32(buf)?;
    let (buf, time) = if additional_header_size >= 4 {
        let (buf, time_value) = le_u32(buf)?;
        (buf, Some(time_value))
    } else {
        (buf, None)
    };
    let (buf, _unknown_remained) = if additional_header_size > 4 {
        let remainder = additional_header_size - 4;
        let (buf, unknown_remained) = take!(buf, remainder)?;
        (buf, Some(unknown_remained))
    } else {
        (buf, None)
    };

    let (buf, data_slice) = take!(buf, payload_size)?;
    let pad_size = match payload_size % PAD_SIZE {
        0 => 0,
        n => PAD_SIZE - n,
    };
    let (buf, _padding) = take!(buf, pad_size)?;
    assert_eq!(payload_size as usize, data_slice.len());

    let video_type = match video_type_str {
        "H264" => VideoType::H264,
        "H265" => VideoType::H265,
        _ => unreachable!(),
    };

    Ok((
        buf,
        BcMediaIframe {
            video_type,
            // payload_size,
            microseconds,
            time,
            data: data_slice.to_vec(),
        },
    ))
}

fn bcmedia_pframe(buf: &[u8]) -> IResult<&[u8], BcMediaPframe> {
    let (buf, video_type_str) = context(
        "Video Type is unrecognised in PFrame",
        verify(take4, |x| matches!(x, "H264" | "H265")),
    )(buf)?;
    let (buf, payload_size) = le_u32(buf)?;
    let (buf, additional_header_size) = le_u32(buf)?;
    let (buf, microseconds) = le_u32(buf)?;
    let (buf, _unknown_b) = le_u32(buf)?;
    let (buf, _additional_header) = take!(buf, additional_header_size)?;
    let (buf, data_slice) = take!(buf, payload_size)?;
    let pad_size = match payload_size % PAD_SIZE {
        0 => 0,
        n => PAD_SIZE - n,
    };
    let (buf, _padding) = take!(buf, pad_size)?;
    assert_eq!(payload_size as usize, data_slice.len());

    let video_type = match video_type_str {
        "H264" => VideoType::H264,
        "H265" => VideoType::H265,
        _ => unreachable!(),
    };

    Ok((
        buf,
        BcMediaPframe {
            video_type,
            // payload_size,
            microseconds,
            data: data_slice.to_vec(),
        },
    ))
}

fn bcmedia_aac(buf: &[u8]) -> IResult<&[u8], BcMediaAac> {
    let (buf, payload_size) = le_u16(buf)?;
    let (buf, _payload_size_b) = le_u16(buf)?;
    let (buf, data_slice) = take!(buf, payload_size)?;
    let pad_size = match payload_size as u32 % PAD_SIZE {
        0 => 0,
        n => PAD_SIZE - n,
    };
    let (buf, _padding) = take!(buf, pad_size)?;

    Ok((
        buf,
        BcMediaAac {
            // payload_size,
            data: data_slice.to_vec(),
        },
    ))
}

fn bcmedia_adpcm(buf: &[u8]) -> IResult<&[u8], BcMediaAdpcm> {
    const SUB_HEADER_SIZE: u16 = 4;

    let (buf, payload_size) = le_u16(buf)?;
    let (buf, _payload_size_b) = le_u16(buf)?;
    let (buf, _magic) = context(
        "ADPCM data magic value is invalid",
        verify(le_u16, |x| *x == MAGIC_HEADER_BCMEDIA_ADPCM_DATA),
    )(buf)?;
    // On some camera this value is just 2
    // On other cameras is half the block size without the header
    let (buf, _half_block_size) = le_u16(buf)?;
    let block_size = payload_size - SUB_HEADER_SIZE;
    let (buf, data_slice) = take!(buf, block_size)?;
    let pad_size = match payload_size as u32 % PAD_SIZE {
        0 => 0,
        n => PAD_SIZE - n,
    };
    let (buf, _padding) = take!(buf, pad_size)?;

    Ok((
        buf,
        BcMediaAdpcm {
            // payload_size,
            // block_size,
            data: data_slice.to_vec(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::bcmedia::model::*;
    use bytes::BytesMut;
    use env_logger::Env;
    use log::*;
    use std::io::ErrorKind;

    fn init() {
        let _ = env_logger::Builder::from_env(Env::default().default_filter_or("info"))
            .is_test(true)
            .try_init();
    }

    #[test]
    // This method will test the decoding on swann cameras output
    //
    // Crucially this contains adpcm
    fn test_swan_deser() {
        init();

        let sample = [
            include_bytes!("samples/video_stream_swan_00.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_01.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_02.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_03.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_04.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_05.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_06.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_07.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_08.raw").as_ref(),
            include_bytes!("samples/video_stream_swan_09.raw").as_ref(),
        ]
        .concat();

        // Should derealise all of this
        loop {
            let e = BcMedia::deserialize(&mut BytesMut::from(&sample[..]));
            match e {
                Err(Error::Io(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                    // Reach end of files
                    break;
                }
                Err(e) => {
                    error!("{:?}", e);
                    panic!();
                }
                Ok(_) => {}
            }
        }
    }

    #[test]
    // This method will test the decoding of argus2 cameras output
    //
    // This packet has an extended iframe
    fn test_argus2_iframe_extended() {
        init();

        let sample = [
            include_bytes!("samples/argus2_iframe_0.raw").as_ref(),
            include_bytes!("samples/argus2_iframe_1.raw").as_ref(),
            include_bytes!("samples/argus2_iframe_2.raw").as_ref(),
            include_bytes!("samples/argus2_iframe_3.raw").as_ref(),
            include_bytes!("samples/argus2_iframe_4.raw").as_ref(),
        ]
        .concat();

        // Should derealise all of this
        loop {
            let e = BcMedia::deserialize(&mut BytesMut::from(&sample[..]));
            match e {
                Err(Error::Io(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                    // Reach end of files
                    break;
                }
                Err(e) => {
                    error!("{:?}", e);
                    panic!();
                }
                Ok(_) => {}
            }
        }
    }

    #[test]
    // This method will test the decoding of argus2 cameras output
    //
    // This packet has an extended pframe
    fn test_argus2_pframe_extended() {
        init();

        let sample = [
            include_bytes!("samples/argus2_pframe_0.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_1.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_2.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_3.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_4.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_5.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_6.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_7.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_8.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_9.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_10.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_11.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_12.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_13.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_14.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_15.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_16.raw").as_ref(),
            include_bytes!("samples/argus2_pframe_17.raw").as_ref(),
        ]
        .concat();

        // Should derealise all of this
        loop {
            let e = BcMedia::deserialize(&mut BytesMut::from(&sample[..]));
            match e {
                Err(Error::Io(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                    // Reach end of files
                    break;
                }
                Err(e) => {
                    error!("{:?}", e);
                    panic!();
                }
                Ok(_) => {}
            }
        }
    }

    #[test]
    // Tests the decoding of an info v1
    fn test_info_v1() {
        init();

        let sample = include_bytes!("samples/info_v1.raw");

        let e = BcMedia::deserialize(&mut BytesMut::from(&sample[..]));
        assert!(matches!(
            e,
            Ok(BcMedia::InfoV1(BcMediaInfoV1 {
                video_width: 2560,
                video_height: 1440,
                fps: 30,
                start_year: 121,
                start_month: 8,
                start_day: 4,
                start_hour: 23,
                start_min: 23,
                start_seconds: 52,
                end_year: 121,
                end_month: 8,
                end_day: 4,
                end_hour: 23,
                end_min: 23,
                end_seconds: 52,
            }))
        ));
    }

    #[test]
    fn test_iframe() {
        init();

        let sample = [
            include_bytes!("samples/iframe_0.raw").as_ref(),
            include_bytes!("samples/iframe_1.raw").as_ref(),
            include_bytes!("samples/iframe_2.raw").as_ref(),
            include_bytes!("samples/iframe_3.raw").as_ref(),
            include_bytes!("samples/iframe_4.raw").as_ref(),
        ]
        .concat();

        let e = BcMedia::deserialize(&mut BytesMut::from(&sample[..]));
        if let Ok(BcMedia::Iframe(BcMediaIframe {
            video_type: VideoType::H264,
            microseconds: 3557705112,
            time: Some(1628085232),
            data: d,
        })) = e
        {
            assert_eq!(d.len(), 192881);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_pframe() {
        init();

        let sample = [
            include_bytes!("samples/pframe_0.raw").as_ref(),
            include_bytes!("samples/pframe_1.raw").as_ref(),
        ]
        .concat();

        let e = BcMedia::deserialize(&mut BytesMut::from(&sample[..]));
        if let Ok(BcMedia::Pframe(BcMediaPframe {
            video_type: VideoType::H264,
            microseconds: 3557767112,
            data: d,
        })) = e
        {
            assert_eq!(d.len(), 45108);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_adpcm() {
        init();

        let sample = include_bytes!("samples/adpcm_0.raw");

        let e = BcMedia::deserialize(&mut BytesMut::from(&sample[..]));
        if let Ok(BcMedia::Adpcm(BcMediaAdpcm { data: d })) = e {
            assert_eq!(d.len(), 244);
        } else {
            panic!();
        }
    }
}

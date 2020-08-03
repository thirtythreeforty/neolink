use log::trace;
use std::cmp::min;
use std::convert::TryInto;

pub const CHUNK_SIZE: usize = 40000;

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub enum MediaDataKind {
    VideoDataIframe,
    VideoDataPframe,
    AudioDataAac,
    AudioDataAdpcm,
    InfoData,
    Invalid,
    Continue,
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MediaData {
    pub data: Vec<u8>,
}

impl MediaData {
    pub fn body(&self) -> &[u8] {
        let lower_limit = self.header_size();
        let upper_limit = self.data_size() + lower_limit;
        let len = self.len();
        if !self.complete() {
            unreachable!(); // An incomplete packet should be discarded not read, panic if we try
        }
        trace!(
            "Sending data from {} to {} of {}",
            lower_limit,
            upper_limit,
            len
        );
        &self.data[lower_limit..upper_limit]
    }

    pub fn complete(&self) -> bool {
        if self.len() < self.header_size() {
            return false;
        }
        let full_data_len = self.len() - self.header_size();
        full_data_len >= self.data_size() && full_data_len < self.data_size() + 8
    }

    pub fn expected_num_packet(&self) -> usize {
        self.data_size() / CHUNK_SIZE + 1
    }

    pub fn header(&self) -> &[u8] {
        let lower_limit = 0;
        let upper_limit = self.header_size() + lower_limit;
        &self.data[min(self.len(), lower_limit)..min(self.len(), upper_limit)]
    }

    pub fn header_size_from_raw(data: &[u8]) -> usize {
        let kind = MediaData::kind_from_raw(data);
        match kind {
            MediaDataKind::VideoDataIframe => 32,
            MediaDataKind::VideoDataPframe => 24,
            MediaDataKind::AudioDataAac => 8,
            MediaDataKind::AudioDataAdpcm => 16,
            MediaDataKind::InfoData => 32,
            MediaDataKind::Unknown | MediaDataKind::Invalid | MediaDataKind::Continue => 0,
        }
    }

    pub fn header_size(&self) -> usize {
        MediaData::header_size_from_raw(&self.data)
    }

    pub fn data_size_from_raw(data: &[u8]) -> usize {
        let kind = MediaData::kind_from_raw(data);
        match kind {
            MediaDataKind::VideoDataIframe => MediaData::bytes_to_size(&data[8..12]),
            MediaDataKind::VideoDataPframe => MediaData::bytes_to_size(&data[8..12]),
            MediaDataKind::AudioDataAac => MediaData::bytes_to_size(&data[4..6]),
            MediaDataKind::AudioDataAdpcm => MediaData::bytes_to_size(&data[4..6]),
            MediaDataKind::InfoData => MediaData::bytes_to_size(&data[4..8]),
            MediaDataKind::Unknown | MediaDataKind::Invalid | MediaDataKind::Continue => {
                data.len()
            }
        }
    }

    pub fn data_size(&self) -> usize {
        MediaData::data_size_from_raw(&self.data)
    }

    pub fn pad_size_from_raw(data: &[u8]) -> usize {
        let data_size = MediaData::data_size_from_raw(data);
        match data_size % 8 {
            0 => 0,
            n => 8 - n,
        }
    }

    pub fn pad_size(&self) -> usize {
        MediaData::data_size_from_raw(&self.data)
    }

    fn bytes_to_size(bytes: &[u8]) -> usize {
        match bytes.len() {
            // 8 Won't fit into usize on a 32-bit machine
            4 => u32::from_le_bytes(bytes.try_into().expect("slice with incorrect length"))
                .try_into()
                .expect("u32 won't fit into usize"),
            2 => u16::from_le_bytes(bytes.try_into().expect("slice with incorrect length"))
                .try_into()
                .expect("u16 won't fit into usize"),
            1 => u8::from_le_bytes(bytes.try_into().expect("slice with incorrect length"))
                .try_into()
                .expect("u8 won't fit into usize"),
            _ => unreachable!(),
        }
    }

    pub fn kind_from_raw(data: &[u8]) -> MediaDataKind {
        if data.len() < 4 {
            return MediaDataKind::Invalid;
        }
        const MAGIC_VIDEO_INFO_V1: &[u8] = &[0x31, 0x30, 0x30, 0x31];
        const MAGIC_VIDEO_INFO_V2: &[u8] = &[0x31, 0x30, 0x30, 0x32];
        const MAGIC_AAC: &[u8] = &[0x30, 0x35, 0x77, 0x62];
        const MAGIC_ADPCM: &[u8] = &[0x30, 0x31, 0x77, 0x62];
        const MAGIC_IFRAME: &[u8] = &[0x30, 0x30, 0x64, 0x63];
        const MAGIC_PFRAME: &[u8] = &[0x30, 0x31, 0x64, 0x63];

        let magic = &data[..4];
        match magic {
            MAGIC_VIDEO_INFO_V1 | MAGIC_VIDEO_INFO_V2 => MediaDataKind::InfoData,
            MAGIC_AAC => MediaDataKind::AudioDataAac,
            MAGIC_ADPCM => MediaDataKind::AudioDataAdpcm,
            MAGIC_IFRAME => MediaDataKind::VideoDataIframe,
            MAGIC_PFRAME => MediaDataKind::VideoDataPframe,
            _ if data.len() == CHUNK_SIZE => MediaDataKind::Continue,
            _ => MediaDataKind::Unknown,
        }
    }

    pub fn kind(&self) -> MediaDataKind {
        MediaData::kind_from_raw(&self.data)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.data.as_slice()
    }
}

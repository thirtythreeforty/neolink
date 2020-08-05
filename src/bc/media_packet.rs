use crate::bc::model::*;
use crate::bc_protocol::connection::BcSubscription;
use err_derive::Error;
use log::trace;
use log::*;
use std::cmp::min;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::time::Duration;

const INVALID_MEDIA_PACKETS: &[MediaDataKind] = &[
    MediaDataKind::Invalid,
    MediaDataKind::Continue,
    MediaDataKind::Unknown,
];

pub const CHUNK_SIZE: usize = 40000;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display = "Timeout")]
    Timeout(#[error(source)] std::sync::mpsc::RecvTimeoutError),
}

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
            MediaDataKind::Unknown | MediaDataKind::Invalid | MediaDataKind::Continue => data.len(),
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

pub struct MediaDataSubscriber<'a> {
    binary_buffer: VecDeque<u8>,
    bc_sub: &'a BcSubscription<'a>,
}

impl<'a> MediaDataSubscriber<'a> {
    pub fn from_bc_sub<'b>(bc_sub: &'b BcSubscription) -> MediaDataSubscriber<'b> {
        MediaDataSubscriber {
            binary_buffer: VecDeque::new(),
            bc_sub: bc_sub,
        }
    }

    fn fill_binary_buffer(&mut self, rx_timeout: Duration) -> Result<()> {
        // Loop messages until we get binary add that data and return
        loop {
            let msg = self.bc_sub.rx.recv_timeout(rx_timeout)?;
            if let BcBody::ModernMsg(ModernMsg {
                binary: Some(binary),
                ..
            }) = msg.body
            {
                // Add the new binary to the buffer and return
                self.binary_buffer.extend(binary);
                break;
            }
        }
        Ok(())
    }

    fn advance_to_media_packet(&mut self, rx_timeout: Duration) -> Result<()> {
        // In the event we get an unknown packet we advance by brute force
        // reading of bytes to the next valid magic
        trace!("Advancing to next know packet header");
        while self.binary_buffer.len() < 4 {
            self.fill_binary_buffer(rx_timeout)?;
        }

        // Check the kind, if its invalid use pop a byte and try again
        let mut magic = MediaDataSubscriber::get_first_n_deque(&self.binary_buffer, 4);
        while INVALID_MEDIA_PACKETS.contains(&MediaData::kind_from_raw(&magic)) {
            self.binary_buffer.pop_front();
            while self.binary_buffer.len() < 4 {
                self.fill_binary_buffer(rx_timeout)?;
            }
            magic = MediaDataSubscriber::get_first_n_deque(&self.binary_buffer, 4);
        }

        Ok(())
    }

    fn get_first_n_deque<T: std::clone::Clone>(deque: &VecDeque<T>, n: usize) -> Vec<T> {
        // NOTE: I want to use make_contiguous
        // This will make this func unneeded as we can use
        // make_contiguous then as_slices.0
        // We won't need the clone in this case either.
        // This is an experimental feature.
        // It is about to be moved to stable though
        // As can be seen from this PR
        // https://github.com/rust-lang/rust/pull/74559
        let slice0 = deque.as_slices().0;
        let slice1 = deque.as_slices().1;
        if slice0.len() >= n {
            return slice0[0..n].iter().cloned().collect();
        } else {
            let remain = n - slice0.len();
            return slice0.iter().chain(&slice1[0..remain]).cloned().collect();
        }
    }

    fn next_media_packet(&mut self, rx_timeout: Duration) -> std::result::Result<MediaData, Error> {
        // Find the first packet (does nothing if already at one)
        self.advance_to_media_packet(rx_timeout)?;

        // Get the magic bytes (guaranteed by advance_to_media_packet)
        let magic = MediaDataSubscriber::get_first_n_deque(&self.binary_buffer, 4);
        let kind = MediaData::kind_from_raw(&magic);

        // Get enough for the full header
        let header_size = MediaData::header_size_from_raw(&magic);
        while self.binary_buffer.len() < header_size {
            self.fill_binary_buffer(rx_timeout)?;
        }

        // Get enough for the full data + 8 byte buffer
        let header = MediaDataSubscriber::get_first_n_deque(&self.binary_buffer, header_size);
        let data_size = MediaData::data_size_from_raw(&header);
        let pad_size = MediaData::pad_size_from_raw(&header);
        let full_size = header_size + data_size + pad_size;
        while self.binary_buffer.len() < full_size {
            self.fill_binary_buffer(rx_timeout)?;
        }

        // Pop the full binary buffer
        let binary = self.binary_buffer.drain(..full_size);

        Ok(MediaData {
            data: binary.collect(),
        })
    }

    pub fn get_media_packet_of_kind(
        &mut self,
        interested_kinds: &[MediaDataKind],
        rx_timeout: Duration,
    ) -> std::result::Result<MediaData, Error> {
        trace!("Finding binary message of interest...");

        let result_media_packet: MediaData;

        // Loop over the messages until we find one we want
        loop {
            let media_packet = self.next_media_packet(rx_timeout)?;
            match media_packet.kind() {
                n if interested_kinds.contains(&n) => {
                    result_media_packet = media_packet;
                    break;
                }
                _ => {
                    trace!(
                        "Ignoring uninteresting binary data kind: {:?}",
                        media_packet.kind()
                    );
                }
            };
        }

        return Ok(result_media_packet);
    }
}
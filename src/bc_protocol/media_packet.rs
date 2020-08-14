use crate::bc::model::*;
use crate::bc_protocol::connection::BcSubscription;
use err_derive::Error;
use log::trace;
use log::*;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::time::Duration;

const INVALID_MEDIA_PACKETS: &[MediaDataKind] = &[MediaDataKind::Unknown];

// MAGIC_SIZE: Number of bytes needed to get magic header type, represets minimum bytes to pull from the
// stream
const MAGIC_SIZE: usize = 4;
// PAD_SIZE: Media packets use 8 byte padding
const PAD_SIZE: usize = 8;

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
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MediaData {
    data: Vec<u8>,
}

impl MediaData {
    pub fn body(&self) -> &[u8] {
        let lower_limit = self.header_size();
        let upper_limit = self.data_size() + lower_limit;
        &self.data[lower_limit..upper_limit]
    }

    fn header_size_from_kind(kind: MediaDataKind) -> usize {
        match kind {
            MediaDataKind::VideoDataIframe => 32,
            MediaDataKind::VideoDataPframe => 24,
            MediaDataKind::AudioDataAac => 8,
            MediaDataKind::AudioDataAdpcm => 16,
            MediaDataKind::InfoData => 32,
            MediaDataKind::Unknown => 0,
        }
    }

    fn header_size_from_raw(data: &[u8]) -> usize {
        let kind = MediaData::kind_from_raw(data);
        MediaData::header_size_from_kind(kind)
    }

    fn header_size(&self) -> usize {
        MediaData::header_size_from_raw(&self.data)
    }

    fn data_size_from_raw(data: &[u8]) -> usize {
        let kind = MediaData::kind_from_raw(data);
        match kind {
            MediaDataKind::VideoDataIframe => MediaData::bytes_to_size(&data[8..12]),
            MediaDataKind::VideoDataPframe => MediaData::bytes_to_size(&data[8..12]),
            MediaDataKind::AudioDataAac => MediaData::bytes_to_size(&data[4..6]),
            MediaDataKind::AudioDataAdpcm => MediaData::bytes_to_size(&data[4..6]),
            MediaDataKind::InfoData => 0, // The bytes in MediaData::bytes_to_size(&data[4..8]) seem to be the size of the header
            MediaDataKind::Unknown => data.len(),
        }
    }

    fn data_size(&self) -> usize {
        MediaData::data_size_from_raw(&self.data)
    }

    fn pad_size_from_raw(data: &[u8]) -> usize {
        let data_size = MediaData::data_size_from_raw(data);
        match data_size % PAD_SIZE {
            0 => 0,
            n => PAD_SIZE - n,
        }
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

    fn kind_from_raw(data: &[u8]) -> MediaDataKind {
        // When calling this ensure you have enough data for header_size +2
        // Else full_header_check_from_kind will fail because we check the
        // First two bytes after the header for the audio stream
        // Since AAC and ADMPC streams start in a predicatble manner
        assert!(
            data.len() >= MAGIC_SIZE,
            "At least four bytes needed to get media packet type"
        );
        const MAGIC_VIDEO_INFO_V1: &[u8] = &[0x31, 0x30, 0x30, 0x31];
        const MAGIC_VIDEO_INFO_V2: &[u8] = &[0x31, 0x30, 0x30, 0x32];
        const MAGIC_AAC: &[u8] = &[0x30, 0x35, 0x77, 0x62];
        const MAGIC_ADPCM: &[u8] = &[0x30, 0x31, 0x77, 0x62];
        const MAGIC_IFRAME: &[u8] = &[0x30, 0x30, 0x64, 0x63];
        const MAGIC_PFRAME: &[u8] = &[0x30, 0x31, 0x64, 0x63];

        let magic = &data[..MAGIC_SIZE];
        match magic {
            MAGIC_VIDEO_INFO_V1 | MAGIC_VIDEO_INFO_V2 => MediaDataKind::InfoData,
            MAGIC_AAC => MediaDataKind::AudioDataAac,
            MAGIC_ADPCM => MediaDataKind::AudioDataAdpcm,
            MAGIC_IFRAME => MediaDataKind::VideoDataIframe,
            MAGIC_PFRAME => MediaDataKind::VideoDataPframe,
            _ => {
                trace!("Unknown magic kind: {:x?}", &magic);
                MediaDataKind::Unknown
            }
        }
    }

    pub fn kind(&self) -> MediaDataKind {
        MediaData::kind_from_raw(&self.data)
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
            bc_sub,
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
        while self.binary_buffer.len() < MAGIC_SIZE {
            self.fill_binary_buffer(rx_timeout)?;
        }

        // Check the kind, if its invalid use pop a byte and try again
        let mut magic = MediaDataSubscriber::get_first_n_deque(&self.binary_buffer, MAGIC_SIZE);
        if INVALID_MEDIA_PACKETS.contains(&MediaData::kind_from_raw(&magic)) {
            warn!("Possibly truncated packet or unknown magic in stream");
            trace!("Unknown magic was: {:x?}", &magic);
        }
        while INVALID_MEDIA_PACKETS.contains(&MediaData::kind_from_raw(&magic)) {
            self.binary_buffer.pop_front();
            while self.binary_buffer.len() < MAGIC_SIZE {
                self.fill_binary_buffer(rx_timeout)?;
            }
            magic = MediaDataSubscriber::get_first_n_deque(&self.binary_buffer, MAGIC_SIZE);
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
            slice0[0..n].to_vec()
        } else {
            let remain = n - slice0.len();
            slice0.iter().chain(&slice1[0..remain]).cloned().collect()
        }
    }

    pub fn next_media_packet(
        &mut self,
        rx_timeout: Duration,
    ) -> std::result::Result<MediaData, Error> {
        // Find the first packet (does nothing if already at one)
        self.advance_to_media_packet(rx_timeout)?;

        // Get the magic bytes (guaranteed by advance_to_media_packet)
        let magic = MediaDataSubscriber::get_first_n_deque(&self.binary_buffer, MAGIC_SIZE);

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
}

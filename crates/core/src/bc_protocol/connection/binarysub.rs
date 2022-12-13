use super::{
    super::{connection::BcSubscription, RX_TIMEOUT},
    BcConnection,
};
use crate::bc::model::*;
use crossbeam_channel::RecvTimeoutError;
use std::io::{BufRead, Error, ErrorKind, Read};

type Result<T> = std::result::Result<T, Error>;

/// A `BinarySubscriber` is a helper util to read the binary stream
/// of a [`BcSubscription`]
///
/// Some messgaes like video streams contain a substream in their
/// payload that may be broken at any point into multiple bc packets.
/// This subscriber will create a stream reader for these binary streams.
///
/// This stream should be accessed via the [`BufRead`] and [`Read`] trait
pub struct BinarySubscriber<'a> {
    bc_sub: &'a BcSubscription<'a>,
    buffer: Vec<u8>,
    consumed: usize,
}

impl<'a> BinarySubscriber<'a> {
    /// Creates a binary subsciber from a BcSubscrption.
    /// When reading the next packet it will skip over multiple
    /// Bc packets to fill the binary buffer so ensure you
    /// only want binary packets when calling read
    pub fn from_bc_sub<'b>(bc_sub: &'b BcSubscription) -> BinarySubscriber<'b> {
        BinarySubscriber {
            bc_sub,
            buffer: vec![],
            consumed: 0,
        }
    }
}

impl<'a> Read for BinarySubscriber<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let buffer = self.fill_buf()?;
        let amt = std::cmp::min(buf.len(), buffer.len());

        // First check if the amount of bytes we want to read is small:
        // `copy_from_slice` will generally expand to a call to `memcpy`, and
        // for a single byte the overhead is significant.
        if amt == 1 {
            buf[0] = buffer[0];
        } else {
            buf[..amt].copy_from_slice(&buffer[..amt]);
        }

        self.consume(amt);

        Ok(amt)
    }
}
impl<'a> BufRead for BinarySubscriber<'a> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        const CLEAR_CONSUMED_AT: usize = 1024;
        // This is a trade off between caching too much dead memory
        // and calling the drain method too often
        if self.consumed > CLEAR_CONSUMED_AT {
            let _ = self.buffer.drain(0..self.consumed).collect::<Vec<u8>>();
            self.consumed = 0;
        }
        while self.buffer.len() <= self.consumed {
            let msg = self
                .bc_sub
                .rx
                .recv_timeout(RX_TIMEOUT)
                .map_err(|err| match err {
                    RecvTimeoutError::Timeout => Error::new(ErrorKind::TimedOut, err),
                    RecvTimeoutError::Disconnected => Error::new(ErrorKind::ConnectionReset, err),
                })?;
            if let BcBody::ModernMsg(ModernMsg {
                payload: Some(BcPayloads::Binary(binary)),
                ..
            }) = msg.body
            {
                // Add the new binary to the buffer and return
                self.buffer.extend(binary);
            }
        }

        Ok(&self.buffer.as_slice()[self.consumed..])
    }

    fn consume(&mut self, amt: usize) {
        assert!(self.consumed + amt <= self.buffer.len());
        self.consumed += amt;
    }
}

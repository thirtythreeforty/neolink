// This is a test module so it will appear as unused most of the time
#![allow(dead_code)]
// This module is used to subscribe to raw bytes on disk
//
// It is mostly used for testing
//
use std::{
    fs,
    io::{BufRead, Error, Read},
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Error>;

/// A `FileSubscriber` is a helper util to read a binary stream
/// from a series of files
///
/// This stream should be accessed via the [`BufRead`] and [`Read`] trait
#[derive(Debug)]
pub struct FileSubscriber {
    files: Vec<PathBuf>,
    buffer: Vec<u8>,
    consumed: usize,
}

impl FileSubscriber {
    /// Creates a binary subsciber from a BcSubscrption.
    /// When reading the next packet it will skip over multiple
    /// Bc packets to fill the binary buffer so ensure you
    /// only want binary packets when calling read
    pub fn from_files<P: AsRef<Path>>(paths: Vec<P>) -> FileSubscriber {
        FileSubscriber {
            files: paths
                .iter()
                .rev()
                .map(|p| {
                    let path: &Path = p.as_ref();
                    path.to_path_buf()
                })
                .collect(),
            buffer: vec![],
            consumed: 0,
        }
    }
}

impl Read for FileSubscriber {
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
impl BufRead for FileSubscriber {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        const CLEAR_CONSUMED_AT: usize = 1024;
        // This is a trade off between caching too much dead memory
        // and calling the drain method too often
        if self.consumed > CLEAR_CONSUMED_AT {
            let _ = self.buffer.drain(0..self.consumed).collect::<Vec<u8>>();
            self.consumed = 0;
        }
        while self.buffer.len() <= self.consumed {
            let next_file = self.files.pop();
            if let Some(next_file) = next_file {
                let bytes = &fs::read(next_file)?;
                self.buffer.extend(bytes);
            } else {
                break;
            }
        }

        Ok(&self.buffer.as_slice()[self.consumed..])
    }

    fn consume(&mut self, amt: usize) {
        assert!(self.consumed + amt <= self.buffer.len());
        self.consumed += amt;
    }
}

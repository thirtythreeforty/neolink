use super::{Error, Result};
use std::io;
use std::io::{BufRead, Error as IoError, ErrorKind, Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::mpsc::{channel, error::TryRecvError, Receiver, Sender};

type IoResult<T> = std::result::Result<T, IoError>;

pub struct TcpSource {
    stream: Arc<TcpStream>,
    tx_out: Sender<Vec<u8>>,
    rx_in: Receiver<Result<Vec<u8>>>,
    rx_out_err: Receiver<Error>,
    buffer: Vec<u8>,
    write_buffer: Vec<u8>,
}

const MTU: usize = 1030;

impl TcpSource {
    pub async fn new(addr: SocketAddr) -> Result<TcpSource> {
        let stream = Arc::new(connect_to(addr).await?);

        // Spawn task that handles incomming bytes
        let (tx_in, rx_in) = channel(100);
        let stream_in = stream.clone();
        tokio::spawn(async move {
            loop {
                match process_inbound(&stream_in).await {
                    Ok(data) => {
                        tx_in.send(Ok(data));
                    }
                    Err(e) => {
                        tx_in.send(Err(e.into()));
                        break;
                    }
                }
            }
        });

        // Spawn task that handles outgoing bytes
        let (tx_out, mut rx_out) = channel(100);
        let (tx_out_err, rx_out_err) = channel(1);
        let stream_out = stream.clone();
        tokio::spawn(async move {
            loop {
                if let Some(data) = rx_out.recv().await {
                    if let Err(e) = process_outbound(&stream_out, &data).await {
                        tx_out_err.send(e).await;
                        break;
                    }
                } else {
                    break;
                }
            }
        });

        let me = Self {
            stream,
            tx_out,
            rx_out_err,
            rx_in,
            buffer: Default::default(),
            write_buffer: Default::default(),
        };
        Ok(me)
    }
}

impl BufRead for TcpSource {
    fn fill_buf(&mut self) -> IoResult<&[u8]> {
        loop {
            match self.rx_in.try_recv() {
                Ok(Ok(data)) => {
                    self.buffer.extend(data);
                }
                Err(TryRecvError::Empty) => {
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    return Err(IoError::from(ErrorKind::ConnectionAborted));
                }
                Err(e) => {
                    return Err(IoError::new(ErrorKind::Other, e));
                }
            }
        }
        Ok(&self.buffer)
    }

    fn consume(&mut self, amt: usize) {
        self.buffer.drain(0..amt).collect::<Vec<u8>>();
    }
}

impl Read for TcpSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
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

impl Write for TcpSource {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        self.write_buffer.extend(buf.to_vec());
        if self.write_buffer.len() > MTU {
            let _ = self.flush();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        for chunk in self.write_buffer.chunks(MTU) {
            if self.tx_out.blocking_send(chunk.to_vec()).is_err() {
                self.write_buffer.clear();
                return Err(IoError::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "Connection dropped",
                ));
            }
        }
        self.write_buffer.clear();
        Ok(())
    }
}

/// Helper to create a TcpStream with a connect timeout
async fn connect_to(addr: SocketAddr) -> Result<TcpStream> {
    let socket = match addr {
        SocketAddr::V4(_) => TcpSocket::new_v4()?,
        SocketAddr::V6(_) => TcpSocket::new_v6()?,
    };

    Ok(socket.connect(addr).await?)
}

async fn process_inbound(stream: &TcpStream) -> Result<Vec<u8>> {
    // Wait for the socket to be readable
    stream.readable().await?;

    let mut buf = Vec::<u8>::with_capacity(4096);

    // Try to read data, this may still fail with `WouldBlock`
    // if the readiness event is a false positive.
    match stream.try_read_buf(&mut buf) {
        Ok(0) => {
            return Ok(Default::default());
        }
        Ok(n) => {
            return Ok(buf);
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
            return Ok(Default::default());
        }
        Err(e) => {
            return Err(e.into());
        }
    }
}

async fn process_outbound(stream: &TcpStream, tx: &Vec<u8>) -> Result<usize> {
    // Wait for the socket to be writable
    loop {
        stream.writable().await?;

        // Try to write data, this may still fail with `WouldBlock`
        // if the readiness event is a false positive.
        match stream.try_write(b"hello world") {
            Ok(n) => {
                return Ok(n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
}

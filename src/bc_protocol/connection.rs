use crate::bc;
use crate::bc::media_packet::*;
use crate::bc::model::*;
use err_derive::Error;
use log::*;
use socket2::{Domain, Socket, Type};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const INVALID_MEDIA_PACKETS: &[MediaDataKind] = &[
    MediaDataKind::Invalid,
    MediaDataKind::Continue,
    MediaDataKind::Unknown,
];

/// A shareable connection to a camera.  Handles serialization of messages.  To send/receive, call
/// .subscribe() with a message ID.  You can use the BcSubscription to send or receive only
/// messages with that ID; each incoming message is routed to its appropriate subscriber.
///
/// There can be only one subscriber per kind of message at a time.
pub struct BcConnection {
    connection: Arc<Mutex<TcpStream>>,
    subscribers: Arc<Mutex<BTreeMap<u32, Sender<Bc>>>>,
    rx_thread: Option<JoinHandle<()>>,
}

pub struct BcSubscription<'a> {
    pub rx: Receiver<Bc>,
    msg_id: u32,
    conn: &'a BcConnection,
    binary_buffer: VecDeque<u8>,
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display = "Communication error")]
    CommunicationError(#[error(source)] std::io::Error),

    #[error(display = "Deserialization error")]
    DeserializationError(#[error(source)] bc::de::Error),

    #[error(display = "Serialization error")]
    SerializationError(#[error(source)] bc::ser::Error),

    #[error(display = "Simultaneous subscription")]
    SimultaneousSubscription { msg_id: u32 },

    #[error(display = "Timeout")]
    Timeout(#[error(source)] std::sync::mpsc::RecvTimeoutError),
}

impl BcConnection {
    pub fn new(addr: SocketAddr, timeout: Duration) -> Result<BcConnection> {
        let tcp_conn = connect_to(addr, timeout)?;
        let subscribers: Arc<Mutex<BTreeMap<u32, Sender<Bc>>>> = Default::default();

        let mut subs = subscribers.clone();
        let conn = tcp_conn.try_clone()?;

        let rx_thread = std::thread::spawn(move || {
            let mut context = BcContext::new();
            while let Ok(_) = BcConnection::poll(&mut context, &conn, &mut subs) {}
        });

        Ok(BcConnection {
            connection: Arc::new(Mutex::new(tcp_conn)),
            subscribers,
            rx_thread: Some(rx_thread),
        })
    }

    pub fn subscribe(&self, msg_id: u32) -> Result<BcSubscription> {
        let (tx, rx) = channel();
        match self.subscribers.lock().unwrap().entry(msg_id) {
            Entry::Vacant(vac_entry) => vac_entry.insert(tx),
            Entry::Occupied(_) => return Err(Error::SimultaneousSubscription { msg_id }),
        };
        Ok(BcSubscription {
            rx,
            conn: self,
            msg_id,
            binary_buffer: VecDeque::new(),
        })
    }

    fn poll(
        context: &mut BcContext,
        connection: &TcpStream,
        subscribers: &mut Arc<Mutex<BTreeMap<u32, Sender<Bc>>>>,
    ) -> Result<()> {
        // Don't hold the lock during deserialization so we don't poison the subscribers mutex if
        // something goes wrong

        let response = Bc::deserialize(context, connection).map_err(|err| {
            // If the connection hangs up, hang up on all subscribers
            subscribers.lock().unwrap().clear();
            err
        })?;
        let msg_id = response.meta.msg_id;

        let mut locked_subs = subscribers.lock().unwrap();
        match locked_subs.entry(msg_id) {
            Entry::Occupied(mut occ) => {
                if let Err(_) = occ.get_mut().send(response) {
                    // Exceedingly unlikely, unless you mishandle the subscription object
                    warn!("Subscriber to ID {} dropped their channel", msg_id);
                    occ.remove();
                }
            }
            Entry::Vacant(_) => {
                debug!("Ignoring uninteresting message ID {}", msg_id);
                trace!("Contents: {:?}", response);
            }
        }

        Ok(())
    }
}

impl Drop for BcConnection {
    fn drop(&mut self) {
        debug!("Shutting down BcConnection...");
        let _ = self.connection.lock().unwrap().shutdown(Shutdown::Both);
        match self
            .rx_thread
            .take()
            .expect("rx_thread join handle should always exist")
            .join()
        {
            Ok(_) => {
                debug!("Shutdown finished OK");
            }
            Err(e) => {
                error!("Receiving thread panicked: {:?}", e);
            }
        }
    }
}

impl<'a> BcSubscription<'a> {
    pub fn send(&self, bc: Bc) -> Result<()> {
        assert!(bc.meta.msg_id == self.msg_id);

        bc.serialize(&*self.conn.connection.lock().unwrap())?;
        Ok(())
    }

    fn fill_binary_buffer(&mut self, rx_timeout: Duration) -> Result<()> {
        // Loop messages until we get binary
        loop {
            let msg = self.rx.recv_timeout(rx_timeout)?;
            if let BcBody::ModernMsg(ModernMsg {
                binary: Some(binary),
                ..
            }) = msg.body
            {
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
        let mut magic = BcSubscription::get_first_n_deque(&self.binary_buffer, 4);
        while INVALID_MEDIA_PACKETS.contains(&MediaData::kind_from_raw(&magic)) {
            self.binary_buffer.pop_front();
            while self.binary_buffer.len() < 4 {
                self.fill_binary_buffer(rx_timeout)?;
            }
            magic = BcSubscription::get_first_n_deque(&self.binary_buffer, 4);
        }

        Ok(())
    }

    fn get_first_n_deque<T: std::clone::Clone>(deque: &VecDeque<T>, n: usize) -> Vec<T> {
        // NOTE: I want to use make_contiguous
        // This will make this func unneeded as we can use
        // make_contiguous then as_slices.0
        // We won't need the clone in this case either.
        // This is an experimental feature.
        // It is able to be moved to stable though
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
        // Get enough data for at least the magic
        while self.binary_buffer.len() < 4 {
            self.fill_binary_buffer(rx_timeout)?;
        }

        // Check the kind, if its invalid use advance to get a valid
        // If not just grab the kind from the buffer
        let mut magic = BcSubscription::get_first_n_deque(&self.binary_buffer, 4);
        trace!("Magic 1: {:x?}", magic);
        trace!("Kind: {:?}", MediaData::kind_from_raw(&magic));
        if INVALID_MEDIA_PACKETS.contains(&MediaData::kind_from_raw(&magic)) {
            // The if is because
            // I presume this is expensive and only call when neccecary
            self.advance_to_media_packet(rx_timeout)?;
            magic = BcSubscription::get_first_n_deque(&self.binary_buffer, 4);
        }
        trace!("Magic 2: {:x?}", magic);
        let kind = MediaData::kind_from_raw(&magic);

        // Get enough for the full header
        let header_size = MediaData::header_size_from_raw(&magic);
        while self.binary_buffer.len() < header_size {
            self.fill_binary_buffer(rx_timeout)?;
        }

        // Get enough for the full data + 8 byte buffer
        let header = BcSubscription::get_first_n_deque(&self.binary_buffer, header_size);
        trace!("Header: {:x?}", header);
        let data_size = MediaData::data_size_from_raw(&header);
        let pad_size = MediaData::pad_size_from_raw(&header);
        let full_size = header_size + data_size + pad_size;
        while self.binary_buffer.len() < full_size {
            self.fill_binary_buffer(rx_timeout)?;
        }
        trace!("data_size: {}", data_size);

        // Pop the full binary buffer
        let binary = self.binary_buffer.drain(..full_size);

        Ok(MediaData {
            data: binary.collect(),
        })
    }

    pub fn get_media_packet(
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
                    trace!("Ignoring uninteresting binary data kind");
                }
            };
        }
        return Ok(result_media_packet);
    }
}

/// Makes it difficult to avoid unsubscribing when you're finished
impl<'a> Drop for BcSubscription<'a> {
    fn drop(&mut self) {
        self.conn.subscribers.lock().unwrap().remove(&self.msg_id);
    }
}

/// Helper to create a TcpStream with a connect timeout
fn connect_to(addr: SocketAddr, timeout: Duration) -> Result<TcpStream> {
    let socket = match addr {
        SocketAddr::V4(_) => Socket::new(Domain::ipv4(), Type::stream(), None)?,
        SocketAddr::V6(_) => {
            let s = Socket::new(Domain::ipv6(), Type::stream(), None)?;
            s.set_only_v6(false)?;
            s
        }
    };

    socket.set_keepalive(Some(timeout))?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;
    socket.connect_timeout(&addr.into(), timeout)?;

    Ok(socket.into_tcp_stream())
}

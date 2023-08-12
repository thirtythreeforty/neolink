use super::DiscoveryResult;
use crate::bc::codex::BcCodex;
use crate::bc::model::*;
use crate::bcudp::codex::BcUdpCodex;
use crate::bcudp::model::*;
use crate::{Credentials, Error, Result};
use delegate::delegate;
use futures::{
    sink::{Sink, SinkExt},
    stream::{IntoAsyncRead, Stream, StreamExt, TryStreamExt},
};
use log::*;
use rand::{seq::SliceRandom, thread_rng};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::{
    net::UdpSocket,
    time::{interval, Duration, Instant, Interval},
};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};
use tokio_util::{
    codec::{Decoder, Encoder, Framed},
    udp::UdpFramed,
};

const MTU: usize = 1350;
const UDPDATA_HEADER_SIZE: usize = 20;

pub(crate) struct UdpSource {
    inner: Framed<Compat<IntoAsyncRead<UdpPayloadSource>>, BcCodex>,
}

impl UdpSource {
    #[allow(unused)]
    pub(crate) async fn new<T: Into<String>, U: Into<String>>(
        addr: SocketAddr,
        client_id: i32,
        camera_id: i32,
        username: T,
        password: Option<U>,
        debug: bool,
    ) -> Result<Self> {
        let stream = Arc::new(connect().await?);

        Self::new_from_socket(
            stream, addr, client_id, camera_id, username, password, debug,
        )
        .await
    }
    pub(crate) async fn new_from_discovery<T: Into<String>, U: Into<String>>(
        discovery: DiscoveryResult,
        username: T,
        password: Option<U>,
        debug: bool,
    ) -> Result<Self> {
        // Ensure that the discovery keep alive are all stopped here
        // We now handle all coms in UdpSource
        Self::new_from_socket(
            discovery.socket,
            discovery.addr,
            discovery.client_id,
            discovery.camera_id,
            username,
            password,
            debug,
        )
        .await
    }

    pub(crate) async fn new_from_socket<T: Into<String>, U: Into<String>>(
        stream: Arc<UdpSocket>,
        addr: SocketAddr,
        client_id: i32,
        camera_id: i32,
        username: T,
        password: Option<U>,
        debug: bool,
    ) -> Result<Self> {
        let bcudp_source = BcUdpSource::new_from_socket(stream, addr).await?;
        let payload_source = bcudp_source.into_payload_source(client_id, camera_id);
        let async_read = payload_source.into_async_read().compat();
        let codex = if debug {
            BcCodex::new_with_debug(Credentials::new(username, password))
        } else {
            BcCodex::new(Credentials::new(username, password))
        };
        let framed = Framed::new(async_read, codex);

        Ok(Self { inner: framed })
    }

    // pub(crate) async fn send(&mut self, bc: Bc) -> Result<()> {
    //     self.inner.send(bc).await
    // }
    // pub(crate) async fn recv(&mut self) -> Result<Bc> {
    //     loop {
    //         if let Some(result) = self.inner.next().await {
    //             return result;
    //         }
    //     }
    // }
}

impl Stream for UdpSource {
    type Item = std::result::Result<<BcCodex as Decoder>::Item, <BcCodex as Decoder>::Error>;

    delegate! {
        to Pin::new(&mut self.inner) {
            fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
        }
    }

    delegate! {
        to self.inner {
            fn size_hint(&self) -> (usize, Option<usize>);
        }
    }
}

impl Sink<Bc> for UdpSource {
    type Error = <BcCodex as Encoder<Bc>>::Error;

    delegate! {
        to Pin::new(&mut self.inner) {
            fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
            fn start_send(mut self: Pin<&mut Self>, item: Bc) -> std::result::Result<(), Self::Error>;
            fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
            fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
        }
    }
}

pub(crate) struct BcUdpSource {
    inner: UdpFramed<BcUdpCodex, Arc<UdpSocket>>,
    addr: SocketAddr,
}

impl BcUdpSource {
    #[allow(unused)]
    pub(crate) async fn new(addr: SocketAddr) -> Result<Self> {
        let stream = Arc::new(connect().await?);

        Self::new_from_socket(stream, addr).await
    }

    #[allow(unused)]
    pub(crate) async fn new_from_discovery(discovery: DiscoveryResult) -> Result<Self> {
        Self::new_from_socket(discovery.socket, discovery.addr).await
    }

    pub(crate) async fn new_from_socket(stream: Arc<UdpSocket>, addr: SocketAddr) -> Result<Self> {
        Ok(Self {
            inner: UdpFramed::new(stream, BcUdpCodex::new()),
            addr,
        })
    }

    pub(crate) fn into_payload_source(self, client_id: i32, camera_id: i32) -> UdpPayloadSource {
        UdpPayloadSource {
            inner: self,
            client_id,
            camera_id,
            packets_sent: 0,
            packets_want: 0,
            sent: Default::default(),
            recieved: Default::default(),
            state: State::Normal,
            send_buffer: Default::default(),
            ack_interval: interval(Duration::from_millis(10)), // Offical Client does ack every 10ms
            resend_interval: interval(Duration::from_millis(500)), // Offical Client does resend every 500ms
            ack_latency: Default::default(),
            flush_state: FlushState::Ready,
        }
    }
}

impl Stream for BcUdpSource {
    type Item = Result<(BcUdp, SocketAddr)>;

    delegate! {
        to Pin::new(&mut self.inner) {
            fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
        }
    }

    delegate! {
        to self.inner {
            fn size_hint(&self) -> (usize, Option<usize>);
        }
    }
}

impl Sink<(BcUdp, SocketAddr)> for BcUdpSource {
    type Error = Error;

    delegate! {
        to Pin::new(&mut self.inner) {
            fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
            fn start_send(mut self: Pin<&mut Self>, item: (BcUdp, SocketAddr)) -> std::result::Result<(), Self::Error>;
            fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
            fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
enum State {
    Normal,   // Normal recieve
    Flushing, // Used to send ack packets and things in the buffer
    Closed,   // Used to shutdown
    YieldNow, // Used to ensure we rest between polling packets so as to not starve the runtime
}

#[derive(Default)]
struct AckLatency {
    current_values: Vec<u32>,
    last_recieve_time: Option<Instant>,
    display_value: u32,
    last_display_time: Option<Instant>,
}

impl AckLatency {
    /// Used to get the current latency, in thd way that the official
    /// client does. This is a value that seems to be updated only every second
    /// Observed values are `0`,    `54785`,    `55062`,     `2528`,
    fn get_value(&self) -> u32 {
        self.display_value
    }

    /// Used to updaet the average latency calculation
    fn feed_ack(&mut self) {
        // Update the last recieve time
        let now = Instant::now();
        if let Some(last_recieve_time) = self.last_recieve_time {
            let diff = (now - last_recieve_time).as_micros();
            self.current_values.push(diff as u32);
            self.last_recieve_time = Some(now);
        } else {
            self.last_recieve_time = Some(now);
        }

        // Update the display_value
        // this is done only ever 1s
        if let Some(last_display_time) = self.last_display_time {
            if now - last_display_time > Duration::from_secs(1) {
                // A second has passed update this
                self.last_display_time = Some(now);
                let current_values_count = self.current_values.len() as u32;
                let current_value = self
                    .current_values
                    .iter()
                    .fold(0u32, |acc, value| acc + *value / current_values_count);
                self.current_values = vec![]; // Reset the average vec

                self.display_value = current_value;
            }
        } else {
            // First 1s is a zero value
            self.last_display_time = Some(now);
            self.display_value = 0;
        }
    }
}

enum FlushState {
    Ready,
    Flush,
}

pub(crate) struct UdpPayloadSource {
    inner: BcUdpSource,
    client_id: i32,
    camera_id: i32,
    packets_sent: u32,
    packets_want: u32,
    sent: BTreeMap<u32, UdpData>,
    recieved: BTreeMap<u32, Vec<u8>>,
    state: State,
    send_buffer: VecDeque<BcUdp>,
    /// Offical Client does ack every 10ms if we don't also do this the camera
    /// seems to think we have a poor connection and will abort
    /// This `ack_interval` controls how ofen we do this
    ack_interval: Interval,
    /// Offical Client does resend every 500ms
    /// This `resend_interval` controls how ofen we do this
    resend_interval: Interval,
    ack_latency: AckLatency,
    flush_state: FlushState,
}

impl UdpPayloadSource {
    fn build_send_ack(&self) -> UdpAck {
        if self.packets_want > 0 {
            let mut first_missing: u32 = self.packets_want;
            while self.recieved.contains_key(&first_missing) {
                // Happens if we have recieved but not consumed yet
                first_missing += 1;
            }
            let missing_ids = if let Some(end) = self.recieved.keys().max() {
                let mut vec = vec![];
                // From last contiguous packet to last recieved packet
                // create a payload of `00` (unreceived) and `01` (received)
                // that can be used to form the `UdpAck` packet
                for i in (first_missing)..(end + 1) {
                    if self.recieved.contains_key(&i) {
                        vec.push(1)
                    } else {
                        vec.push(0)
                    }
                }
                vec
            } else {
                vec![]
            };

            UdpAck {
                connection_id: self.camera_id,
                packet_id: first_missing - 1, // Last we actually have is first_missing - 1
                group_id: 0,
                maybe_latency: self.ack_latency.get_value(),
                payload: missing_ids,
            }
        } else {
            UdpAck::empty(self.camera_id)
        }
    }

    fn handle_ack(&mut self, ack: UdpAck) {
        let start = ack.packet_id;
        if start != 0xffffffff {
            // -1 means havent got anything yet
            self.sent.retain(|&k, _| k > start);

            for (idx, &value) in ack.payload.iter().enumerate() {
                let packet_id = (start + 1) + idx as u32;
                if value > 0 {
                    self.sent.remove(&packet_id);
                }
            }
        }
        self.ack_latency.feed_ack();
    }

    fn maintanence(&mut self, cx: &mut Context<'_>) {
        // Check for periodic resends
        // log::info!("Maintaince");
        if self.resend_interval.poll_tick(cx).is_ready() {
            for (_, resend) in self.sent.iter() {
                self.send_buffer.push_back(BcUdp::Data(resend.clone()));
            }
        }
        if self.ack_interval.poll_tick(cx).is_ready()
        // && self.packets_want > 0
        {
            // log::info!("Pushing Ack");
            let ack = BcUdp::Ack(self.build_send_ack());
            self.send_buffer.push_back(ack);
            self.state = State::Flushing;
        }
    }
}

impl Stream for UdpPayloadSource {
    type Item = IoResult<Vec<u8>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let camera_addr = self.inner.addr;
        let this = self.get_mut();
        // log::info!("this.state: {:?}", this.state);
        match this.state {
            State::Normal => {
                // this.state = State::YieldNow;
                // Handle resend events
                this.maintanence(cx);
                // Data ready to go
                if let Some(payload) = this.recieved.remove(&this.packets_want) {
                    this.packets_want += 1;
                    // error!("packets_want: {}", this.packets_want);
                    return Poll::Ready(Some(Ok(payload)));
                }
                // Normal behaviors
                match this.inner.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok((
                        BcUdp::Data(UdpData {
                            connection_id,
                            packet_id,
                            payload,
                        }),
                        addr,
                    )))) if connection_id == this.client_id
                        && addr == camera_addr
                        && packet_id >= this.packets_want =>
                    {
                        if packet_id == this.packets_want {
                            this.packets_want += 1;
                            // error!("packets_want: {}", this.packets_want);
                            let ack = BcUdp::Ack(this.build_send_ack());
                            this.send_buffer.push_back(ack);
                            return Poll::Ready(Some(Ok(payload)));
                        } else {
                            this.recieved.insert(packet_id, payload);
                        }
                    }
                    Poll::Ready(Some(Ok((
                        BcUdp::Data(UdpData {
                            connection_id,
                            packet_id,
                            ..
                        }),
                        addr,
                    )))) if connection_id == this.client_id
                        && addr == camera_addr
                        && packet_id < this.packets_want =>
                    {
                        trace!("UdpPayloadSource.RecievedPacket: Old data: {}", packet_id);
                    }
                    Poll::Ready(Some(Ok((
                        BcUdp::Ack(ack @ UdpAck { connection_id, .. }),
                        addr,
                    )))) if connection_id == this.client_id && addr == camera_addr => {
                        this.handle_ack(ack);
                        // Rather then immediatly flush wait for the next call
                        // this.state = State::Flushing;
                    }
                    Poll::Ready(Some(Err(e))) => {
                        return Poll::Ready(Some(Err(IoError::new(ErrorKind::Other, e))));
                    }
                    Poll::Ready(None) => {
                        return Poll::Ready(None);
                    }
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                    Poll::Ready(Some(Ok((bcudp, addr)))) => {
                        trace!(
                            "UdpPayloadSource.RecievedPacker: UnexpectedData {:?} from {}",
                            bcudp,
                            addr
                        );
                    } // _ => {
                      //     trace!("UDPSource.RecievedPacker: Other?");
                      //     // Repeat/unintersting packet
                      // }
                }
            }
            State::Flushing => {
                // log::info!("Fushing: {}", this.send_buffer.len());
                if this.send_buffer.is_empty() {
                    this.state = State::Normal;
                } else {
                    match this.poll_flush_unpin(cx) {
                        Poll::Ready(Ok(())) => {
                            this.state = State::Normal;
                        }
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Some(Err(e)));
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
            }
            State::Closed => {
                return Poll::Ready(Some(Err(IoError::from(ErrorKind::ConnectionAborted))));
            }
            State::YieldNow => {
                this.state = State::Normal;
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl Sink<Vec<u8>> for UdpPayloadSource {
    type Error = IoError;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        let this = self.get_mut();
        this.inner
            .poll_ready_unpin(cx)
            .map_err(|e| IoError::new(ErrorKind::Other, e))
    }
    fn start_send(self: Pin<&mut Self>, item: Vec<u8>) -> std::result::Result<(), Self::Error> {
        let this = self.get_mut();
        for chunk in item.chunks(MTU - UDPDATA_HEADER_SIZE) {
            let udp_data = UdpData {
                connection_id: this.camera_id,
                packet_id: this.packets_sent,
                payload: chunk.to_vec(),
            };
            this.packets_sent += 1;
            this.send_buffer.push_back(BcUdp::Data(udp_data));
        }
        // Push through whats on the buffer
        for first in this.send_buffer.drain(..) {
            if let BcUdp::Data(data) = &first {
                let id = data.packet_id;
                this.sent.insert(id, data.clone());
            }
            let addr = this.inner.addr;
            Pin::new(&mut this.inner)
                .start_send((first, addr))
                .map_err(|e| IoError::new(ErrorKind::Other, e))?;
        }
        Ok(())
    }
    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        // log::info!("poll_flush");
        let this = self.get_mut();
        let res = match this.flush_state {
            FlushState::Ready => match this.poll_ready_unpin(cx) {
                Poll::Ready(Ok(())) => {
                    for first in this.send_buffer.drain(..) {
                        if let BcUdp::Data(data) = &first {
                            let id = data.packet_id;
                            this.sent.insert(id, data.clone());
                        }
                        let addr = this.inner.addr;
                        Pin::new(&mut this.inner)
                            .start_send((first, addr))
                            .map_err(|e| IoError::new(ErrorKind::Other, e))?;
                    }
                    this.flush_state = FlushState::Flush;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                poll => poll,
            },
            FlushState::Flush => {
                match this
                    .inner
                    .poll_flush_unpin(cx)
                    .map_err(|e| IoError::new(ErrorKind::Other, e))
                {
                    Poll::Ready(Ok(())) => {
                        this.flush_state = FlushState::Ready;
                        Poll::Ready(Ok(()))
                    }
                    poll => poll,
                }
            }
        };

        log::trace!("poll_flush end: {:?}", res);
        res
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        let this = self.get_mut();
        if let State::Closed = this.state {
            return Poll::Ready(Ok(()));
        }

        match this.poll_flush_unpin(cx) {
            Poll::Ready(Ok(())) => {
                this.state = State::Closed;
                this.inner
                    .poll_close_unpin(cx)
                    .map_err(|e| IoError::new(ErrorKind::Other, e))
            }
            poll => poll,
        }
    }
}

impl futures::AsyncWrite for UdpPayloadSource {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<IoResult<usize>> {
        let mut this = self.get_mut();
        match Pin::new(&mut this).poll_ready(cx) {
            Poll::Ready(Ok(())) => match Pin::new(&mut this).start_send(buf.to_vec()) {
                Ok(()) => Poll::Ready(Ok(buf.len())),
                Err(e) => Poll::Ready(Err(e)),
            },
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<()>> {
        Sink::poll_flush(self, cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<()>> {
        Sink::poll_close(self, cx)
    }
}

/// Helper to create a UdpStream
async fn connect() -> Result<UdpSocket> {
    let mut ports: Vec<u16> = (53500..54000).collect();
    let mut rng = thread_rng();
    ports.shuffle(&mut rng);

    let addrs: Vec<_> = ports
        .iter()
        .map(|&port| SocketAddr::from(([0, 0, 0, 0], port)))
        .collect();
    let socket = UdpSocket::bind(&addrs[..]).await?;

    Ok(socket)
}

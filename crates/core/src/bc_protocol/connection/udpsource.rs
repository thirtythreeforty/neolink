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
use rand::{seq::SliceRandom, thread_rng};
use std::collections::BTreeMap;
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc::channel;
use tokio::{
    net::UdpSocket,
    task::JoinHandle,
    time::{interval, sleep, Duration, Instant, Interval},
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};
use tokio_util::sync::{CancellationToken, PollSender};
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
        let payload_source = bcudp_source.into_payload_source(client_id, camera_id).await;
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

    pub(crate) async fn into_payload_source(
        self,
        client_id: i32,
        camera_id: i32,
    ) -> UdpPayloadSource {
        UdpPayloadSource::new(self, client_id, camera_id).await
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

pub(crate) struct UdpPayloadSource {
    inner_stream: ReceiverStream<IoResult<Vec<u8>>>,
    inner_sink: PollSender<Vec<u8>>,
    handle: JoinHandle<Result<()>>,
    cancel_token: CancellationToken,
}

impl Drop for UdpPayloadSource {
    fn drop(&mut self) {
        self.cancel_token.cancel();
        self.handle.abort();
    }
}

struct UdpPayloadInner {
    camera_addr: SocketAddr,
    ack_tx: PollSender<UdpAck>,
    socket_in: PollSender<BcUdp>,
    socket_out: ReceiverStream<(BcUdp, SocketAddr)>,
    thread_stream: PollSender<IoResult<Vec<u8>>>,
    thread_sink: ReceiverStream<Vec<u8>>,
    client_id: i32,
    camera_id: i32,
    packets_sent: u32,
    packets_want: u32,
    sent: BTreeMap<u32, UdpData>,
    recieved: BTreeMap<u32, Vec<u8>>,
    /// Offical Client does ack every 10ms if we don't also do this the camera
    /// seems to think we have a poor connection and will abort
    /// This `ack_interval` controls how ofen we do this
    /// Offical Client does resend every 500ms
    /// This `resend_interval` controls how ofen we do this
    resend_interval: Interval,
    ack_latency: AckLatency,
    cancel: CancellationToken,
}
impl UdpPayloadInner {
    fn new(
        inner: BcUdpSource,
        thread_stream: PollSender<IoResult<Vec<u8>>>,
        thread_sink: ReceiverStream<Vec<u8>>,
        client_id: i32,
        camera_id: i32,
    ) -> Self {
        let camera_addr = inner.addr;
        let cancel = CancellationToken::new();
        // Data in this needs to be passed into the socket regularly
        // especially the ACK packets on UDP. The thread must not lock
        // and MUST send ACK packets or else be dropped by the camera.
        // In order to achieve this we use dedicated threads for ACK
        // and the socket

        let (socket_in_tx, socket_in_rx) = channel::<BcUdp>(1000);
        let (socket_out_tx, socket_out_rx) = channel::<(BcUdp, SocketAddr)>(1000);
        let (mut socket_tx, mut socket_rx) = inner.split();

        // Send to socket
        let send_cancel = cancel.clone();
        let mut socket_in_rx = ReceiverStream::new(socket_in_rx);
        let thread_camera_addr = camera_addr;
        tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let result = tokio::select! {
                    _ = send_cancel.cancelled() => {
                        Result::Ok(())
                    },
                    v = async {
                        while let Some(packet) = socket_in_rx.next().await {
                            socket_tx.send((packet, thread_camera_addr)).await?;
                        }
                        Ok(())
                    } => v,
                };
                send_cancel.cancel();
                result
            })
        });

        // Queue up ack packets
        let ack_cancel = cancel.clone();
        let mut ack_interval = interval(Duration::from_millis(100)); // Offical Client does ack every 10ms
        ack_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let (ack_tx, mut ack_rx) = channel(1000);
        let thread_camera_id = camera_id;
        let mut ack_socket_in_tx = PollSender::new(socket_in_tx.clone());
        tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                tokio::select! {
                    _ = ack_cancel.cancelled() => {
                        Result::Ok(())
                    },
                    v = async {
                        let mut ack_packet = UdpAck::empty(thread_camera_id);
                        loop {
                            tokio::select! {
                                v = ack_rx.recv() => {
                                    // Update the ACK packet
                                    if let Some(v) = v {
                                        ack_packet = v;
                                        Ok(())
                                    } else {
                                        Err(Error::DroppedConnection)
                                    }
                                },
                                _ = ack_interval.tick() => {
                                    // Send an ack packet
                                    log::trace!("send ack");
                                    ack_socket_in_tx.send(BcUdp::Ack(ack_packet.clone())).await?;
                                    Ok(())
                                }
                            }?;
                        }
                    } => v,
                }
            })
        });

        // Get from socket
        let get_cancel = cancel.clone();
        let mut socket_out_tx = PollSender::new(socket_out_tx);
        tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            // A packet of ANY kind must be recieved in the 10 last second
            // This is based on the official client which also seems to take 10s timeout
            const TIME_OUT: u64 = 10;
            let mut recv_timeout = Box::pin(sleep(Duration::from_secs(TIME_OUT)));

            runtime.block_on(async {
                let res = tokio::select! {
                    _ = get_cancel.cancelled() => {
                        Result::Ok(())
                    },
                    v = async {
                        loop {
                            let packet = tokio::select! {
                                v = async {
                                    socket_rx.next().await.ok_or(Error::DroppedConnection)
                                } => {
                                    log::trace!("Got packet");
                                    recv_timeout.as_mut().reset(Instant::now() + Duration::from_secs(TIME_OUT));
                                    v
                                }
                                _ = recv_timeout.as_mut() => {
                                    log::trace!("Timeout");
                                    Err(Error::DroppedConnection)
                                }
                            }??;
                            // let packet = socket_rx.next().await.ok_or(Error::DroppedConnection)??;
                            socket_out_tx.send(packet).await?;
                        }
                    } => v,
                };
                log::trace!("GetSocket: {res:?}");
                res
            })?;

            Result::Ok(())
        });

        Self {
            camera_addr,
            ack_tx: PollSender::new(ack_tx),
            socket_in: PollSender::new(socket_in_tx),
            socket_out: ReceiverStream::new(socket_out_rx),
            thread_stream,
            thread_sink,
            client_id,
            camera_id,
            packets_sent: 0,
            packets_want: 0,
            sent: Default::default(),
            recieved: Default::default(),
            resend_interval: interval(Duration::from_millis(500)), // Offical Client does resend every 500ms
            ack_latency: Default::default(),
            cancel,
        }
    }
    async fn run(&mut self) -> Result<()> {
        let camera_addr = self.camera_addr;
        tokio::select! {
            _ = self.resend_interval.tick() => {
                log::trace!("Resend Tick");
                for (_, resend) in self.sent.iter() {
                    self.socket_in.feed(BcUdp::Data(resend.clone())).await?;
                }
                self.ack_tx.send(self.build_send_ack()).await?; // Ensure we update this sometimes too
                Result::Ok(())
            },
            v = self.thread_sink.next() => {
                log::trace!("App->Camera");
                // Incomming from application
                // Outgoing on socket
                let item = v.ok_or(Error::DroppedConnection)?;

                for chunk in item.chunks(MTU - UDPDATA_HEADER_SIZE) {
                    let udp_data = UdpData {
                        connection_id: self.camera_id,
                        packet_id: self.packets_sent,
                        payload: chunk.to_vec(),
                    };
                    self.packets_sent += 1;
                    self.sent.insert(udp_data.packet_id, udp_data.clone());
                    self.socket_in.feed(BcUdp::Data(udp_data)).await?;
                }
                Ok(())
            }
            v = self.socket_out.next() => {
                log::trace!("Camera->App");
                // Incomming from socket
                // Outgoing to application
                let (item, addr) = v.ok_or(Error::DroppedConnection)?;
                if addr == camera_addr {
                    match item {
                        BcUdp::Discovery(_disc) => {},
                        BcUdp::Ack(ack) => {
                            if ack.connection_id == self.client_id {
                                self.handle_ack(ack);
                            }
                        },
                        BcUdp::Data(data)  => {
                            if data.connection_id == self.client_id {
                                let packet_id = data.packet_id;
                                if packet_id >= self.packets_want {
                                    // error!("packets_want: {}", this.packets_want);
                                    self.recieved.insert(packet_id, data.payload);
                                    self.ack_tx.send(self.build_send_ack()).await?;
                                }
                            }
                        },
                    }
                }
                log::trace!("Got packet");
                Ok(())
            },
        }?;
        log::trace!("Send");
        while let Some(payload) = self.recieved.remove(&self.packets_want) {
            log::trace!("  + {}", self.packets_want);
            self.packets_want += 1;
            self.thread_stream.send(Ok(payload)).await?;
        }
        log::trace!("Flush");
        self.socket_in.flush().await?;
        log::trace!("Flushed");
        Ok(())
    }

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
}

impl Drop for UdpPayloadInner {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl UdpPayloadSource {
    async fn new(inner: BcUdpSource, client_id: i32, camera_id: i32) -> Self {
        let (inner_sink, thread_sink) = channel(1000);
        let (thread_stream, inner_stream) = channel(1000);

        let mut payload_inner = UdpPayloadInner::new(
            inner,
            PollSender::new(thread_stream),
            ReceiverStream::new(thread_sink),
            client_id,
            camera_id,
        );
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let thread_cancel_token = cancel_token.clone();
        let handle = tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            runtime.block_on(async move {
                tokio::select! {
                    v = async {
                        loop {
                            if payload_inner.thread_stream.is_closed() {
                                payload_inner.thread_sink.close();
                                return Err(Error::DroppedConnection);
                            }
                            log::trace!("Calling inner");
                            let res = payload_inner.run().await;
                            log::trace!("Called inner: {:?}", res);
                            match res {
                                Ok(()) => {}
                                Err(e) => {
                                    log::trace!("UDP Error. Connection will Drop: {:?}", e);
                                    // Pass error up
                                    let _ = payload_inner
                                        .thread_stream
                                        .send(Err(IoError::new(ErrorKind::Other, e.clone())))
                                        .await;
                                    return Result::<()>::Err(e);
                                }
                            }
                        }
                    } => v,
                    _ = thread_cancel_token.cancelled() => Ok(()),
                }
            })
        });

        UdpPayloadSource {
            inner_stream: ReceiverStream::new(inner_stream),
            inner_sink: PollSender::new(inner_sink),
            handle,
            cancel_token,
        }
    }
}

impl Stream for UdpPayloadSource {
    type Item = IoResult<Vec<u8>>;

    delegate! {
        to Pin::new(&mut self.inner_stream) {
            fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
        }
    }

    delegate! {
        to self.inner_stream {
            fn size_hint(&self) -> (usize, Option<usize>);
        }
    }
}

impl Sink<Vec<u8>> for UdpPayloadSource {
    type Error = IoError;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        self.get_mut()
            .inner_sink
            .poll_ready_unpin(cx)
            .map_err(|e| IoError::new(ErrorKind::Other, e))
    }
    fn start_send(self: Pin<&mut Self>, item: Vec<u8>) -> std::result::Result<(), Self::Error> {
        self.get_mut()
            .inner_sink
            .start_send_unpin(item)
            .map_err(|e| IoError::new(ErrorKind::Other, e))
    }
    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        self.get_mut()
            .inner_sink
            .poll_flush_unpin(cx)
            .map_err(|e| IoError::new(ErrorKind::Other, e))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        self.get_mut()
            .inner_sink
            .poll_close_unpin(cx)
            .map_err(|e| IoError::new(ErrorKind::Other, e))
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

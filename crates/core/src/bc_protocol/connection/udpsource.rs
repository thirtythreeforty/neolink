use super::DiscoveryResult;
use crate::bc::codex::BcCodex;
use crate::bc::model::*;
use crate::bcudp::codex::BcUdpCodex;
use crate::bcudp::{model::*, xml::*};
use crate::{Credentials, Error, Result};
use delegate::delegate;
use futures::{
    sink::{Sink, SinkExt},
    stream::{IntoAsyncRead, Stream, StreamExt, TryStreamExt},
};
use rand::{seq::SliceRandom, thread_rng, Rng};
use std::collections::BTreeMap;
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::{
    net::UdpSocket,
    sync::mpsc::channel,
    task::JoinSet,
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

pub(crate) type InnerFramed = Framed<Compat<IntoAsyncRead<UdpPayloadSource>>, BcCodex>;
pub(crate) struct UdpSource {
    inner: Pin<Box<InnerFramed>>,
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
        discovery.socket.set_broadcast(false)?;
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

        Ok(Self {
            inner: Box::pin(framed),
        })
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
        to self.inner.as_mut() {
            fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
        }
    }

    delegate! {
        to self.inner.as_ref().get_ref() {
            fn size_hint(&self) -> (usize, Option<usize>);
        }
    }
}

impl Sink<Bc> for UdpSource {
    type Error = <BcCodex as Encoder<Bc>>::Error;

    delegate! {
        to self.inner.as_mut() {
            fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
            fn start_send(mut self: Pin<&mut Self>, item: Bc) -> std::result::Result<(), Self::Error>;
            fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
            fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>>;
        }
    }
}

pub(crate) struct BcUdpSource {
    inner: Pin<Box<UdpFramed<BcUdpCodex, Arc<UdpSocket>>>>,
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
            inner: Box::pin(UdpFramed::new(stream, BcUdpCodex::new())),
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
        to self.inner.as_mut() {
            fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
        }
    }

    delegate! {
        to self.inner.as_ref().get_ref() {
            fn size_hint(&self) -> (usize, Option<usize>);
        }
    }
}

impl Sink<(BcUdp, SocketAddr)> for BcUdpSource {
    type Error = Error;

    delegate! {
        to self.inner.as_mut() {
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
    inner_stream: Pin<Box<ReceiverStream<IoResult<Vec<u8>>>>>,
    inner_sink: PollSender<Vec<u8>>,
    set: JoinSet<Result<()>>,
    cancel_token: CancellationToken,
}

impl Drop for UdpPayloadSource {
    fn drop(&mut self) {
        log::trace!("Drop UdpPayloadSource");
        self.cancel_token.cancel();
        let _gt = tokio::runtime::Handle::current().enter();
        let mut set = std::mem::take(&mut self.set);
        tokio::task::spawn(async move {
            while set.join_next().await.is_some() {}
            log::trace!("Dropped UdpPayloadSource");
        });
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
    set: JoinSet<Result<()>>,
}
impl UdpPayloadInner {
    fn new(
        mut inner: BcUdpSource,
        thread_stream: PollSender<IoResult<Vec<u8>>>,
        thread_sink: ReceiverStream<Vec<u8>>,
        client_id: i32,
        camera_id: i32,
    ) -> Self {
        let mut set = JoinSet::new();
        let camera_addr = inner.addr;
        let cancel = CancellationToken::new();
        // Data in this needs to be passed into the socket regularly
        // especially the ACK packets on UDP. The thread must not lock
        // and MUST send ACK packets or else be dropped by the camera.
        // In order to achieve this we use dedicated threads for ACK
        // and the socket

        let (socket_in_tx, socket_in_rx) = channel::<BcUdp>(100);
        let (socket_out_tx, socket_out_rx) = channel::<(BcUdp, SocketAddr)>(100);
        // let (mut socket_tx, mut socket_rx) = inner.split();

        // Send/Recv on the socket
        let send_cancel = cancel.clone();
        let mut socket_in_rx = ReceiverStream::new(socket_in_rx);
        let thread_camera_addr = camera_addr;
        let mut socket_out_tx = PollSender::new(socket_out_tx);
        let thread_client_id = client_id;
        let thread_camera_id = camera_id;
        const TIME_OUT: u64 = 10;
        let mut recv_timeout = Box::pin(sleep(Duration::from_secs(TIME_OUT)));
        set.spawn(async move {
            let result = tokio::select! {
                _ = send_cancel.cancelled() => {
                    Result::Ok(())
                },
                v = async {
                    loop {
                        break tokio::select!{
                            _ = recv_timeout.as_mut() => {
                                log::trace!("DroppedConnection: Timeout");
                                Err(Error::DroppedConnection)
                            }
                            packet = inner.next() => {
                                log::trace!("Cam->App");
                                let packet = packet.ok_or(Error::DroppedConnection)??;
                                recv_timeout.as_mut().reset(Instant::now() + Duration::from_secs(TIME_OUT));
                                // let packet = socket_rx.next().await.ok_or(Error::DroppedConnection)??;
                                socket_out_tx.send(packet).await?;
                                continue;
                            },
                            packet = socket_in_rx.next() => {
                                let packet = packet.ok_or(Error::DroppedConnection)?;
                                match tokio::time::timeout(tokio::time::Duration::from_millis(250), inner.send((packet, thread_camera_addr))).await {
                                    Ok(written) => {
                                        written?;
                                    }
                                    Err(_) => {
                                        // Socket is (maybe) broken
                                        // Seems to happen with network reconnects like over
                                        // a lossy cellular network
                                        log::debug!("Quick reconnect: Due to socket timeout");
                                        let stream = Arc::new(connect_try_port(inner.inner.get_ref().local_addr()?.port()).await?);
                                        inner = BcUdpSource::new_from_socket(stream, inner.addr).await?;

                                        // Inform the camera that we are the same client
                                        //
                                        // At least I think that is what this is for.
                                        // Might also have to do this for the relay but not sure
                                        let msg = BcUdp::Discovery(UdpDiscovery {
                                            tid: {
                                                let mut rng = thread_rng();
                                                (rng.gen::<u8>()) as u32
                                            },
                                            payload: UdpXml {
                                                c2d_hb: Some(C2dHb {
                                                    cid: thread_client_id,
                                                    did: thread_camera_id,
                                                }),
                                                ..Default::default()
                                            },
                                        });
                                        let _ = tokio::time::timeout(Duration::from_millis(250), inner.send((msg, thread_camera_addr))).await;
                                    }
                                }

                                log::trace!("Send Packet");
                                continue;
                            }
                        }
                    }?;
                    Ok(())
                } => v,
            };
            log::debug!("UdpPayloadInner::new SendToSocket Cancel");
            send_cancel.cancel();
            result
        });

        // Queue up ack packets
        let ack_cancel = cancel.clone();
        let mut ack_interval = interval(Duration::from_millis(10)); // Offical Client does ack every 10ms
        ack_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let (ack_tx, mut ack_rx) = channel(100);
        let thread_camera_id = camera_id;
        let ack_socket_in_tx = socket_in_tx.clone();
        set.spawn(async move {
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
                                    log::trace!("ack_rx.recv() Error::DroppedConnection");
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
        });

        // Queue up Hb packets
        let thread_client_id = client_id;
        let thread_camera_id = camera_id;
        let thread_sender = socket_in_tx.clone();
        let mut thread_interval = interval(Duration::from_secs(1));
        let thread_cancel = cancel.clone();
        set.spawn(async move {
            tokio::select! {
                _ = thread_cancel.cancelled() => Result::Ok(()),
                v = async {
                    loop {
                        thread_interval.tick().await;
                        let msg = BcUdp::Discovery(UdpDiscovery {
                            tid: {
                                let mut rng = thread_rng();
                                (rng.gen::<u8>()) as u32
                            },
                            payload: UdpXml {
                                c2d_hb: Some(C2dHb {
                                    cid: thread_client_id,
                                    did: thread_camera_id,
                                }),
                                ..Default::default()
                            },
                        });
                        if thread_sender.send(msg).await.is_err() {
                            break Result::Ok(());
                        }
                    }
                } => v,
            }
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
            set,
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
                self.ack_tx.feed(self.build_send_ack()).await?; // Ensure we update the ack packet sometimes too
                Result::Ok(())
            },
            v = self.thread_sink.next() => {
                log::trace!("App->Camera");
                // Incomming from application
                // Outgoing on socket
                if v.is_none() {
                    log::trace!("DroppedConnection: self.thread_sink.next(): {:?}", v);
                }
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
                if v.is_none() {
                    log::trace!("DroppedConnection: self.socket_out.next()");
                }
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
                                    self.ack_tx.feed(self.build_send_ack()).await?;
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
            self.thread_stream.feed(Ok(payload)).await?;
        }
        log::trace!("Flush");
        self.socket_in.flush().await?;
        self.thread_stream.flush().await?;
        self.ack_tx.flush().await?;
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
        log::trace!("Drop UdpPayloadInner");
        self.cancel.cancel();
        let _gt = tokio::runtime::Handle::current().enter();
        let mut set = std::mem::take(&mut self.set);
        tokio::task::spawn(async move {
            while set.join_next().await.is_some() {}
            log::trace!("Dropped UdpPayloadInner");
        });
    }
}
impl UdpPayloadSource {
    async fn new(inner: BcUdpSource, client_id: i32, camera_id: i32) -> Self {
        let (inner_sink, thread_sink) = channel(100);
        let (thread_stream, inner_stream) = channel(100);

        let mut payload_inner = UdpPayloadInner::new(
            inner,
            PollSender::new(thread_stream),
            ReceiverStream::new(thread_sink),
            client_id,
            camera_id,
        );
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let thread_cancel_token = cancel_token.clone();
        let mut set = JoinSet::new();
        set.spawn(async move {
            tokio::select! {
                v = async {
                    loop {
                        if payload_inner.thread_stream.is_closed() {
                            log::trace!("payload_inner.thread_stream.is_closed");
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
        });

        UdpPayloadSource {
            inner_stream: Box::pin(ReceiverStream::new(inner_stream)),
            inner_sink: PollSender::new(inner_sink),
            set,
            cancel_token,
        }
    }
}

impl Stream for UdpPayloadSource {
    type Item = IoResult<Vec<u8>>;

    delegate! {
        to self.inner_stream.as_mut() {
            fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
        }
    }

    delegate! {
        to self.inner_stream.as_ref().get_ref() {
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
    {
        let mut rng = thread_rng();
        ports.shuffle(&mut rng);
        drop(rng); // Do not hold RNG over an await
    }

    let addrs: Vec<_> = ports
        .iter()
        .map(|&port| SocketAddr::from(([0, 0, 0, 0], port)))
        .collect();
    let socket = UdpSocket::bind(&addrs[..]).await?;

    Ok(socket)
}

async fn connect_try_port(port: u16) -> Result<UdpSocket> {
    let mut ports: Vec<u16> = (53500..54000).collect();
    {
        let mut rng = thread_rng();
        ports.shuffle(&mut rng);
        drop(rng); // Do not hold RNG over an await
    }

    let addrs: Vec<_> = vec![port]
        .iter()
        .chain(ports.iter())
        .map(|&port| SocketAddr::from(([0, 0, 0, 0], port)))
        .collect();
    let socket = UdpSocket::bind(&addrs[..]).await?;

    Ok(socket)
}

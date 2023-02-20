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
use std::task::{Context, Poll};
use tokio::{
    net::UdpSocket,
    time::{interval, Duration, Interval},
};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};
use tokio_util::{
    codec::{Decoder, Encoder, Framed},
    udp::UdpFramed,
};

const MTU: usize = 1030;
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
    ) -> Result<Self> {
        let stream = connect().await?;

        Self::new_from_socket(stream, addr, client_id, camera_id, username, password).await
    }
    pub(crate) async fn new_from_discovery<T: Into<String>, U: Into<String>>(
        discovery: DiscoveryResult,
        username: T,
        password: Option<U>,
    ) -> Result<Self> {
        Self::new_from_socket(
            discovery.socket,
            discovery.addr,
            discovery.client_id,
            discovery.camera_id,
            username,
            password,
        )
        .await
    }

    pub(crate) async fn new_from_socket<T: Into<String>, U: Into<String>>(
        stream: UdpSocket,
        addr: SocketAddr,
        client_id: i32,
        camera_id: i32,
        username: T,
        password: Option<U>,
    ) -> Result<Self> {
        let bcudp_source = BcUdpSource::new_from_socket(stream, addr).await?;
        let payload_source = bcudp_source.into_payload_source(client_id, camera_id);
        let async_read = payload_source.into_async_read().compat();
        let framed = Framed::new(
            async_read,
            BcCodex::new(Credentials::new(username, password)),
        );

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
    inner: UdpFramed<BcUdpCodex, UdpSocket>,
    addr: SocketAddr,
}

impl BcUdpSource {
    #[allow(unused)]
    pub(crate) async fn new(addr: SocketAddr) -> Result<Self> {
        let stream = connect().await?;

        Self::new_from_socket(stream, addr).await
    }

    #[allow(unused)]
    pub(crate) async fn new_from_discovery(discovery: DiscoveryResult) -> Result<Self> {
        Self::new_from_socket(discovery.socket, discovery.addr).await
    }

    pub(crate) async fn new_from_socket(stream: UdpSocket, addr: SocketAddr) -> Result<Self> {
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
            interval: interval(Duration::from_millis(10)), // Offical Client does ack every 10ms
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

enum State {
    Normal,
    Flushing,
    Closed,
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
    /// This `intveral` controls how ofen we do this
    interval: Interval,
}

impl UdpPayloadSource {
    fn build_send_ack(&self) -> UdpAck {
        assert!(self.packets_want > 0);
        let start: u32 = self.packets_want - 1;
        let missing_ids = if let Some(end) = self.recieved.keys().max() {
            let mut vec = vec![];
            // From last contiguous packet to last recieved packet
            // create a payload of `00` (unreceived) and `01` (received)
            // that can be used to form the `UdpAck` packet
            for i in (start + 1)..(end + 1) {
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
            packet_id: start,
            payload: missing_ids,
        }
    }

    fn handle_ack(&mut self, ack: UdpAck) {
        let start = ack.packet_id;
        self.sent.retain(|&k, _| k > start);

        for (idx, &value) in ack.payload.iter().enumerate() {
            let packet_id = (start + 1) + idx as u32;
            if value > 0 {
                self.sent.remove(&packet_id);
            }
        }
    }
}

impl Stream for UdpPayloadSource {
    type Item = IoResult<Vec<u8>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let camera_addr = self.inner.addr;
        let mut this = self.get_mut();
        loop {
            match this.state {
                State::Normal => {
                    trace!("UDPSource.State: Normal");
                    // Data ready to go
                    if let Some(payload) = this.recieved.remove(&this.packets_want) {
                        trace!("UDPSource.Data: PreReady: {}", this.packets_want);
                        this.packets_want += 1;
                        return Poll::Ready(Some(Ok(payload)));
                    }
                    // Check for periodic resends
                    if this.interval.poll_tick(cx).is_ready() {
                        trace!("UDPSource.RecievedPacker: Resend");
                        for (_, resend) in this.sent.iter() {
                            this.send_buffer.push_back(BcUdp::Data(resend.clone()));
                        }
                        if this.packets_want == 0 {
                            // When no packets have been recieved the
                            // ACK is different specifically unknown_b
                            // is set to -1 and packet_id is also set to
                            // -1
                            // TODO Add this style of ACK
                        } else {
                            let ack = BcUdp::Ack(this.build_send_ack());
                            this.send_buffer.push_back(ack);
                        }
                        this.state = State::Flushing;
                        continue;
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
                                trace!("UDPSource.RecievedPacker: NewData: {}", this.packets_want);
                                this.packets_want += 1;
                                return Poll::Ready(Some(Ok(payload)));
                            } else {
                                trace!("UDPSource.RecievedPacker: OtherData");
                                this.recieved.insert(packet_id, payload);
                            }
                        }
                        Poll::Ready(Some(Ok((
                            BcUdp::Ack(ack @ UdpAck { connection_id, .. }),
                            addr,
                        )))) if connection_id == this.client_id && addr == camera_addr => {
                            trace!("UDPSource.RecievedPacket: Ack");
                            this.handle_ack(ack);
                            // Rather then immediatly flush wait for the next call
                            // this.state = State::Flushing;
                        }
                        Poll::Ready(Some(Err(e))) => {
                            trace!("UDPSource.RecievedPacker: Error");
                            return Poll::Ready(Some(Err(IoError::new(ErrorKind::Other, e))));
                        }
                        Poll::Ready(None) => {
                            trace!("UDPSource.RecievedPacker: Empty");
                            return Poll::Ready(None);
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                        Poll::Ready(Some(Ok((bcudp, addr)))) => {
                            trace!(
                                "UDPSource.RecievedPacker: UnexpectedData {:?} from {}",
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
                    trace!("UDPSource.State: Flushing");
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
                State::Closed => {
                    trace!("UDPSource.State: Closed");
                    return Poll::Ready(Some(Err(IoError::from(ErrorKind::ConnectionAborted))));
                }
            }
        }
    }
}

impl Sink<Vec<u8>> for UdpPayloadSource {
    type Error = IoError;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        let this = self.get_mut();
        match this.state {
            State::Normal => this
                .inner
                .poll_ready_unpin(cx)
                .map_err(|e| IoError::new(ErrorKind::Other, e)),
            State::Flushing => this
                .inner
                .poll_flush_unpin(cx)
                .map_err(|e| IoError::new(ErrorKind::Other, e)),
            State::Closed => Poll::Ready(Err(IoError::from(ErrorKind::ConnectionAborted))),
        }
    }
    fn start_send(self: Pin<&mut Self>, item: Vec<u8>) -> std::result::Result<(), Self::Error> {
        let mut this = self.get_mut();
        for chunk in item.chunks(MTU - UDPDATA_HEADER_SIZE) {
            let udp_data = UdpData {
                connection_id: this.camera_id,
                packet_id: this.packets_sent,
                payload: chunk.to_vec(),
            };
            this.packets_sent += 1;
            this.send_buffer.push_back(BcUdp::Data(udp_data));
        }
        if let Some(first) = this.send_buffer.pop_front() {
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
        let this = self.get_mut();
        loop {
            match this
                .inner
                .poll_flush_unpin(cx)
                .map_err(|e| IoError::new(ErrorKind::Other, e))
            {
                Poll::Ready(Ok(())) => {
                    if let Some(next) = this.send_buffer.pop_front() {
                        match this
                            .inner
                            .poll_ready_unpin(cx)
                            .map_err(|e| IoError::new(ErrorKind::Other, e))
                        {
                            Poll::Ready(Ok(())) => {
                                if let BcUdp::Data(data) = &next {
                                    let id = data.packet_id;
                                    this.sent.insert(id, data.clone());
                                }

                                let addr = this.inner.addr;
                                Pin::new(&mut this.inner)
                                    .start_send((next, addr))
                                    .map_err(|e| IoError::new(ErrorKind::Other, e))?;
                            }
                            poll => {
                                return poll;
                            }
                        }
                    } else {
                        return Poll::Ready(Ok(()));
                    }
                }
                poll => {
                    return poll;
                }
            }
        }
    }
    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        let mut this = self.get_mut();
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

/// Helper to create a TcpStream with a connect timeout
async fn connect() -> Result<UdpSocket> {
    let mut ports: Vec<u16> = (53500..54000).into_iter().collect();
    let mut rng = thread_rng();
    ports.shuffle(&mut rng);

    let addrs: Vec<_> = ports
        .iter()
        .map(|&port| SocketAddr::from(([0, 0, 0, 0], port)))
        .collect();
    let socket = UdpSocket::bind(&addrs[..]).await?;

    Ok(socket)
}

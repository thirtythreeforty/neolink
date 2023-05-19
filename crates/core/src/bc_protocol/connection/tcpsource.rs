use crate::bc::model::*;
use crate::Result;
use crate::{bc::codex::BcCodex, Credentials};
use delegate::delegate;
use futures::{sink::Sink, stream::Stream};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::net::{TcpSocket, TcpStream};
use tokio_util::codec::{Decoder, Encoder, Framed};

pub(crate) struct TcpSource {
    inner: Framed<TcpStream, BcCodex>,
}

impl TcpSource {
    pub(crate) async fn new<T: Into<String>, U: Into<String>>(
        addr: SocketAddr,
        username: T,
        password: Option<U>,
    ) -> Result<TcpSource> {
        let stream = connect_to(addr).await?;

        Ok(Self {
            inner: Framed::new(stream, BcCodex::new(Credentials::new(username, password))),
        })
    }
}

impl Stream for TcpSource {
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

impl Sink<Bc> for TcpSource {
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

/// Helper to create a TcpStream with a connect timeout
async fn connect_to(addr: SocketAddr) -> Result<TcpStream> {
    let socket = match addr {
        SocketAddr::V4(_) => TcpSocket::new_v4()?,
        SocketAddr::V6(_) => TcpSocket::new_v6()?,
    };

    Ok(socket.connect(addr).await?)
}

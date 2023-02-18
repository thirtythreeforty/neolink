//! Trait shared by anything for the camera source.
//!
//! This trait represents the shared interface between
//! TCP and UDP sources
//!
use super::{TcpSource, UdpSource};
use crate::{bc::model::*, Result};
use async_trait::async_trait;

#[async_trait]
pub trait Source:
    futures::stream::Stream<Item = Result<Bc>>
    + futures::sink::Sink<Bc, Error = crate::Error>
    + Send
    + Sync
{
    async fn send(&mut self, bc: Bc) -> Result<()>;
    async fn recv(&mut self) -> Result<Bc>;
}

#[async_trait]
impl Source for TcpSource {
    async fn send(&mut self, bc: Bc) -> Result<()> {
        TcpSource::send(self, bc).await
    }
    async fn recv(&mut self) -> Result<Bc> {
        TcpSource::recv(self).await
    }
}

#[async_trait]
impl Source for UdpSource {
    async fn send(&mut self, bc: Bc) -> Result<()> {
        UdpSource::send(self, bc).await
    }
    async fn recv(&mut self) -> Result<Bc> {
        UdpSource::recv(self).await
    }
}

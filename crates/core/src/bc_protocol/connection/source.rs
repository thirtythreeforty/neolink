//! Trait shared by anything for the camera source.
//!
//! This trait represents the shared interface between
//! TCP and UDP sources
//!
use super::{TcpSource, UdpSource};
use crate::{bc::model::*, Result};
use async_trait::async_trait;

#[async_trait]
pub trait Source: Send + Sync {
    async fn send(&mut self, bc: Bc) -> Result<()>;
    async fn recv(&mut self) -> Result<Bc>;
    fn get_encrypted(&self) -> &EncryptionProtocol;
    fn set_encrypted(&mut self, protocol: EncryptionProtocol);
}

#[async_trait]
impl Source for TcpSource {
    async fn send(&mut self, bc: Bc) -> Result<()> {
        TcpSource::send(self, bc).await
    }
    async fn recv(&mut self) -> Result<Bc> {
        TcpSource::recv(self).await
    }

    fn get_encrypted(&self) -> &EncryptionProtocol {
        TcpSource::get_encrypted(self)
    }
    fn set_encrypted(&mut self, protocol: EncryptionProtocol) {
        TcpSource::set_encrypted(self, protocol)
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

    fn get_encrypted(&self) -> &EncryptionProtocol {
        UdpSource::get_encrypted(self)
    }
    fn set_encrypted(&mut self, protocol: EncryptionProtocol) {
        UdpSource::set_encrypted(self, protocol)
    }
}

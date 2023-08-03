//! This module handles connections and subscribers
//!
//! This includes a tcp and udp connections. As well
//! as subscribers to binary streams that are encoded
//! in the bc packets.
//!
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

mod bcconn;
mod bcsub;
mod discovery;
mod tcpsource;
mod udpsource;

pub(crate) use self::{
    bcconn::BcConnection, bcconn::*, bcsub::BcSubscription, discovery::Discovery,
    tcpsource::TcpSource, udpsource::UdpSource,
};

pub(crate) struct DiscoveryResult {
    socket: Arc<UdpSocket>,
    addr: SocketAddr,
    client_id: i32,
    camera_id: i32,
}

impl DiscoveryResult {
    /// Get the address discovered
    pub(crate) fn get_addr(&self) -> &SocketAddr {
        &self.addr
    }
}

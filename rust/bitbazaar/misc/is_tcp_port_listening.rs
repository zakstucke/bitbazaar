use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    time::Duration,
};

use crate::prelude::*;

/// Check if a port is listening for a given ipv4 address and port.
pub fn is_tcp_port_listening(host: &str, port: u16) -> Result<bool, AnyErr> {
    let timeout_duration = Duration::from_secs(1); // Timeout duration set to 1 second

    let ip = if host == "localhost" {
        Ipv4Addr::LOCALHOST
    } else {
        host.parse::<Ipv4Addr>().change_context(AnyErr)?
    };

    let socket_addr = SocketAddr::V4(SocketAddrV4::new(ip, port));

    match TcpStream::connect_timeout(&socket_addr, timeout_duration) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

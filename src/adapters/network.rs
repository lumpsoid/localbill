//! Connectivity probe via a short-timeout TCP connect.

use std::net::TcpStream;
use std::time::Duration;

use crate::ports::Network;

pub struct StdNetwork;

impl Network for StdNetwork {
    /// `true` when a TCP connection to Cloudflare's public DNS can be
    /// established within 500 ms.
    fn has_internet(&self) -> bool {
        let addr = "1.1.1.1:53".parse().expect("static address is valid");
        TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
    }
}

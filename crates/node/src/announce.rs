//! "I am here": one small UDP datagram every few seconds, so the dashboard's Find button lists
//! every node on the LAN without anyone typing an address.
//!
//! The datagram is JSON, `{"grow":1,"name":"shelf-1","listen":"192.168.0.20:7474"}`, sent to the
//! broadcast address and to loopback on [`PORT`]. Nothing is received here; nothing is secret
//! in it. A node listening on loopback still announces, so a dashboard on the same machine finds
//! it; a dashboard elsewhere sees the loopback address and knows it needs a tunnel.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const PORT: u16 = 7475;
pub const EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub grow: u8,
    pub name: String,
    pub listen: String,
}

impl Hello {
    pub fn new(name: &str, listen: &str) -> Self {
        Self { grow: 1, name: name.into(), listen: listen.into() }
    }
}

/// Send forever. Errors are swallowed: a LAN without broadcast is not a reason to stop growing.
pub fn run(name: &str, listen: &str) {
    let Ok(sock) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else { return };
    let _ = sock.set_broadcast(true);
    let body = serde_json::to_vec(&Hello::new(name, listen)).unwrap_or_default();
    let targets = [SocketAddrV4::new(Ipv4Addr::BROADCAST, PORT), SocketAddrV4::new(Ipv4Addr::LOCALHOST, PORT)];
    loop {
        for t in targets {
            let _ = sock.send_to(&body, t);
        }
        std::thread::sleep(EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_is_small_and_round_trips() {
        let h = Hello::new("shelf-1", "127.0.0.1:7474");
        let bytes = serde_json::to_vec(&h).unwrap();
        assert!(bytes.len() < 200);
        let back: Hello = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, h);
        assert_eq!(back.grow, 1);
    }
}

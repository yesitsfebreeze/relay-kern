use std::net::UdpSocket;
use std::sync::Arc;

use crate::base::constants::{GOSSIP_DISCOVERY_INTERVAL, GOSSIP_DISCOVERY_MULTICAST};

use super::node::Node;

const ANNOUNCE_PREFIX: &str = "kern:";

pub fn start_broadcast(node: &Arc<Node>, port: u16) {
	let node = node.clone();
	let addr = format!("{GOSSIP_DISCOVERY_MULTICAST}:{port}");
	tokio::spawn(async move {
		let socket = match UdpSocket::bind("0.0.0.0:0") {
			Ok(s) => s,
			Err(_) => return,
		};

		let payload = format!("{ANNOUNCE_PREFIX}{}:{}", node.network_id, node.addr());
		let payload_bytes = payload.as_bytes();

		let mut interval = tokio::time::interval(GOSSIP_DISCOVERY_INTERVAL);
		let mut stop = node.stop_rx.clone();
		loop {
			tokio::select! {
				_ = interval.tick() => {
					let _ = socket.send_to(payload_bytes, &addr);
				}
				_ = stop.changed() => break,
			}
		}
	});
}

pub fn parse_announce(s: &str) -> Option<(String, String)> {
	let s = s.strip_prefix(ANNOUNCE_PREFIX)?;
	if s.len() < 38 {
		return None;
	}
	let network_id = &s[..36];
	if s.as_bytes()[36] != b':' {
		return None;
	}
	let tcp_addr = &s[37..];
	Some((network_id.to_string(), tcp_addr.to_string()))
}

//! LAN discovery of RustDesk computers and Wake-on-LAN.
//!
//! Port of upstream `src/lan.rs` (client side): broadcast a `PeerDiscovery { cmd: "ping" }`
//! rendezvous message to UDP port 21119 (`RENDEZVOUS_PORT + 3`); every RustDesk host with
//! "Enable LAN discovery" on (the default) answers `cmd: "pong"` with its ID, hostname, user,
//! platform and MAC address. Wake-on-LAN sends the standard magic packet (6 x 0xFF, then the MAC
//! 16 times) as a UDP broadcast, like upstream `send_wol`.

use hbb_common::{config::RENDEZVOUS_PORT, protobuf::Message as _, rendezvous_proto::*, ResultType};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanPeer {
    pub id: String,
    pub hostname: String,
    pub username: String,
    pub platform: String,
    /// "aa:bb:cc:dd:ee:ff" as the host reported it (may be empty).
    pub mac: String,
    pub ip: IpAddr,
}

fn discovery_port() -> u16 {
    (RENDEZVOUS_PORT + 3) as u16
}

/// Same rule as upstream `is_valid_untrusted_peer_id`: a short run of ID characters.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn clip(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(64).collect()
}

fn ping_bytes() -> ResultType<Vec<u8>> {
    let mut msg = RendezvousMessage::new();
    msg.set_peer_discovery(PeerDiscovery {
        cmd: "ping".to_owned(),
        ..Default::default()
    });
    Ok(msg.write_to_bytes()?)
}

/// Parse one discovery reply; `None` for anything that is not a valid pong.
pub fn parse_pong(bytes: &[u8], from: IpAddr) -> Option<LanPeer> {
    let msg = RendezvousMessage::parse_from_bytes(bytes).ok()?;
    let Some(rendezvous_message::Union::PeerDiscovery(p)) = msg.union else {
        return None;
    };
    if p.cmd != "pong" || !valid_id(&p.id) {
        return None;
    }
    Some(LanPeer {
        id: p.id,
        hostname: clip(&p.hostname),
        username: clip(&p.username),
        platform: clip(&p.platform),
        mac: parse_mac(&p.mac).map(format_mac).unwrap_or_default(),
        ip: from,
    })
}

/// Broadcast a discovery ping and collect answers for `wait` (blocking).
pub fn discover(wait: Duration) -> ResultType<Vec<LanPeer>> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))?;
    socket.set_broadcast(true)?;
    let ping = ping_bytes()?;
    let port = discovery_port();
    socket.send_to(&ping, SocketAddr::from((Ipv4Addr::BROADCAST, port)))?;
    let deadline = Instant::now() + wait;
    // A second ping halfway through catches hosts that missed the first datagram.
    let mut resent = false;
    let mut found: BTreeMap<String, LanPeer> = BTreeMap::new();
    let mut buf = [0u8; 2048];
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        if !resent && deadline - now < wait / 2 {
            resent = true;
            socket
                .send_to(&ping, SocketAddr::from((Ipv4Addr::BROADCAST, port)))
                .ok();
        }
        socket.set_read_timeout(Some((deadline - now).min(Duration::from_millis(200))))?;
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(p) = parse_pong(&buf[..n], from.ip()) {
                    found.entry(p.id.clone()).or_insert(p);
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let mut v: Vec<LanPeer> = found.into_values().collect();
    v.sort_by(|a, b| {
        a.hostname
            .to_lowercase()
            .cmp(&b.hostname.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    Ok(v)
}

/// Six bytes from "aa:bb:cc:dd:ee:ff", "aa-bb-…" or "aabbccddeeff".
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 12
        || s.chars()
            .any(|c| !(c.is_ascii_hexdigit() || c == ':' || c == '-'))
    {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    if mac == [0; 6] || mac == [0xff; 6] {
        return None;
    }
    Some(mac)
}

pub fn format_mac(m: [u8; 6]) -> String {
    m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

pub fn magic_packet(mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0xffu8; 6];
    for _ in 0..16 {
        p.extend_from_slice(&mac);
    }
    p
}

/// Send a Wake-on-LAN magic packet for `mac` (broadcast to UDP ports 9 and 7).
pub fn wake(mac: &str) -> ResultType<()> {
    let mac = parse_mac(mac).ok_or_else(|| hbb_common::anyhow::anyhow!("invalid MAC address"))?;
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))?;
    socket.set_broadcast(true)?;
    let pkt = magic_packet(mac);
    for port in [9u16, 7] {
        socket.send_to(&pkt, SocketAddr::from((Ipv4Addr::BROADCAST, port)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pong(id: &str, mac: &str) -> Vec<u8> {
        let mut msg = RendezvousMessage::new();
        msg.set_peer_discovery(PeerDiscovery {
            cmd: "pong".to_owned(),
            id: id.to_owned(),
            hostname: "office-pc".to_owned(),
            username: "anna".to_owned(),
            platform: "Windows".to_owned(),
            mac: mac.to_owned(),
            ..Default::default()
        });
        msg.write_to_bytes().unwrap()
    }

    #[test]
    fn pong_is_parsed_and_validated() {
        let ip: IpAddr = "192.168.1.20".parse().unwrap();
        let p = parse_pong(&pong("123456789", "AA-BB-CC-DD-EE-01"), ip).unwrap();
        assert_eq!(p.id, "123456789");
        assert_eq!(p.hostname, "office-pc");
        assert_eq!(p.mac, "aa:bb:cc:dd:ee:01");
        assert_eq!(p.ip, ip);
        assert!(parse_pong(&pong("bad id!", ""), ip).is_none());
        assert!(parse_pong(b"garbage", ip).is_none());
        // Our own ping is not a peer.
        assert!(parse_pong(&ping_bytes().unwrap(), ip).is_none());
    }

    #[test]
    fn magic_packet_layout() {
        let mac = parse_mac("01:23:45:67:89:ab").unwrap();
        let p = magic_packet(mac);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xff; 6]);
        assert_eq!(&p[6..12], &mac);
        assert_eq!(&p[96..102], &mac);
        assert!(parse_mac("00:00:00:00:00:00").is_none());
        assert!(parse_mac("zz:23:45:67:89:ab").is_none());
        assert!(parse_mac("0123456789ab").is_some());
    }

    #[test]
    fn discovery_finds_a_local_responder() {
        // A fake host answering on loopback: exercises send/receive without a network.
        let host = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = host.local_addr().unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.send_to(&ping_bytes().unwrap(), addr).unwrap();
        let mut buf = [0u8; 512];
        let (n, from) = host.recv_from(&mut buf).unwrap();
        let msg = RendezvousMessage::parse_from_bytes(&buf[..n]).unwrap();
        assert!(
            matches!(msg.union, Some(rendezvous_message::Union::PeerDiscovery(ref p)) if p.cmd == "ping")
        );
        host.send_to(&pong("987654321", "aa:bb:cc:00:11:22"), from)
            .unwrap();
        let (n, from) = client.recv_from(&mut buf).unwrap();
        let p = parse_pong(&buf[..n], from.ip()).unwrap();
        assert_eq!(p.id, "987654321");
    }
}

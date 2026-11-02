//! Online status of computers, as the RustDesk client shows it in its lists.
//!
//! Port of upstream `client::peer_online::query_online_states`: one TCP connection to the ID
//! server's port 21115 (rendezvous port − 1), an `OnlineRequest` with the IDs, and an
//! `OnlineResponse` whose `states` bytes hold one bit per ID, most significant bit first.

use crate::profile::ServerProfile;
use hbb_common::{
    anyhow::{anyhow, bail},
    config::{CONNECT_TIMEOUT, RENDEZVOUS_PORT},
    protobuf::Message as _,
    rendezvous_proto::*,
    socket_client::{check_port, connect_tcp},
    ResultType,
};

const QUERY_TIMEOUT_MS: u64 = 3_000;

/// One bit per ID from the response's `states`; `None` when it is too short.
pub fn parse_states(states: &[u8], n: usize) -> Option<Vec<bool>> {
    if states.len() < n.div_ceil(8) {
        return None;
    }
    Some((0..n).map(|i| states[i / 8] & (0x80 >> (i % 8)) != 0).collect())
}

/// The online server for an ID server address (`host[:port]`): same host, port − 1.
fn online_server(id_server: &str) -> ResultType<String> {
    let s = check_port(id_server, RENDEZVOUS_PORT);
    let (host, port) = s
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("invalid ID server address {s}"))?;
    let port: u16 = port.parse()?;
    if port == 0 {
        bail!("invalid ID server address {s}");
    }
    Ok(format!("{host}:{}", port - 1))
}

/// Ask the profile's ID server which of `ids` are online (same order as `ids`).
pub async fn query(profile: &ServerProfile, my_id: &str, ids: &[String]) -> ResultType<Vec<bool>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let servers = profile.rendezvous_servers();
    let first = servers
        .first()
        .ok_or_else(|| anyhow!("no ID server configured"))?;
    let mut socket = connect_tcp(online_server(first)?, CONNECT_TIMEOUT).await?;
    let mut msg = RendezvousMessage::new();
    msg.set_online_request(OnlineRequest {
        id: my_id.to_owned(),
        peers: ids.to_vec(),
        ..Default::default()
    });
    socket.send(&msg).await?;
    // Newer servers may open with a KeyExchange first; skip it, like upstream.
    for _ in 0..3 {
        let Some(Ok(bytes)) = socket.next_timeout(QUERY_TIMEOUT_MS).await else {
            bail!("no answer from the online server");
        };
        let Ok(msg) = RendezvousMessage::parse_from_bytes(&bytes) else {
            continue;
        };
        if let Some(rendezvous_message::Union::OnlineResponse(r)) = msg.union {
            return parse_states(&r.states, ids.len()).ok_or_else(|| anyhow!("short online response"));
        }
    }
    bail!("no online response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_read_msb_first() {
        assert_eq!(
            parse_states(&[0b1010_0000, 0b1000_0000], 9),
            Some(vec![true, false, true, false, false, false, false, false, true])
        );
        assert_eq!(parse_states(&[0xff], 9), None);
        assert_eq!(parse_states(&[], 0), Some(vec![]));
    }

    #[test]
    fn online_port_is_rendezvous_minus_one() {
        assert_eq!(online_server("rs.example.com").unwrap(), "rs.example.com:21115");
        assert_eq!(online_server("10.0.0.5:31116").unwrap(), "10.0.0.5:31115");
    }
}

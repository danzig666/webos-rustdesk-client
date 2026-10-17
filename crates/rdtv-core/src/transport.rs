//! Connection establishment: rendezvous lookup, TCP hole punch, relay fallback and the
//! peer key exchange.
//!
//! Ported from RustDesk 1.5.0 (`src/client.rs`: `Client::_start`, `_start_inner`, `connect`,
//! `secure_connection`, `request_relay`, `create_relay`; `src/common.rs`: `key_exchange`,
//! `get_rs_pk`, `decode_id_pk_dtls`, `create_symmetric_key_msg`,
//! `get_next_nonkeyexchange_msg`). Copyright (c) RustDesk contributors, AGPL-3.0.
//!
//! Deliberate reductions versus upstream (documented in docs/DECISIONS.md):
//! * No WebRTC, UDP/KCP punch or IPv6 punch transports. The classic TCP punch + relay
//!   paths are what every stock host since 1.1 accepts; upstream keeps them as fallbacks.
//! * No WebSocket rendezvous (`use_ws`) and no SOCKS proxy UI (hbb_common still honours a
//!   proxy if one is configured in its own config file).
//!
//! Trust behaviour is unchanged: a peer whose signed id cannot be verified with the
//! configured server key falls back exactly as upstream does, and the result reports
//! whether the channel is end-to-end encrypted so the UI can show it.

use crate::profile::ServerProfile;
use hbb_common::{
    anyhow::{anyhow, bail, Context},
    config::{CONNECT_TIMEOUT, READ_TIMEOUT, RELAY_PORT, RENDEZVOUS_PORT},
    protobuf::Message as _,
    rendezvous_proto::*,
    socket_client::{check_port, connect_tcp, connect_tcp_local, ipv4_to_ipv6},
    sodiumoxide::crypto::{box_, secretbox, sign},
    tokio::time::timeout,
    AddrMangle, ResultType, Stream,
};
use base::message_proto::*;
use std::{net::SocketAddr, time::Instant};

/// Version string announced to rendezvous servers and hosts. This client implements the
/// RustDesk 1.5.0 classic transport/login behaviour it was ported from.
pub const PROTOCOL_VERSION: &str = "1.5.0";

/// How the session reached the peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Direct TCP after a rendezvous hole punch.
    DirectPunch,
    /// Direct TCP to an IP address or host:port typed by the user.
    DirectAddress,
    /// Through the relay server.
    Relay,
}

impl Route {
    pub fn label(&self) -> &'static str {
        match self {
            Route::DirectPunch => "Direct (P2P)",
            Route::DirectAddress => "Direct (address)",
            Route::Relay => "Relayed",
        }
    }
}

pub struct Established {
    pub stream: Stream,
    pub route: Route,
    /// True when the peer's signed key was verified and a session key was negotiated.
    pub secured: bool,
    /// Public key of the peer from the rendezvous server, when known.
    pub peer_pk: Option<Vec<u8>>,
}

/// What the user typed in the "ID or address" field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Id(String),
    /// IP address or `host:port`, used without the rendezvous server.
    Address(String),
}

impl Target {
    /// Parse using upstream's rules (`hbb_common::is_ip_str` / `is_domain_port_str`).
    /// Spaces inside IDs are ignored, as the upstream UI does.
    pub fn parse(input: &str) -> Option<Target> {
        let s = input.trim();
        if s.is_empty() {
            return None;
        }
        if hbb_common::is_ip_str(s) || hbb_common::is_domain_port_str(s) {
            return Some(Target::Address(s.to_owned()));
        }
        let id: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        if id.is_empty() || id.len() > 64 {
            return None;
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '@' || c == '.')
        {
            return None;
        }
        Some(Target::Id(id))
    }

    pub fn as_str(&self) -> &str {
        match self {
            Target::Id(s) | Target::Address(s) => s,
        }
    }
}

pub(crate) fn get_pk(pk: &[u8]) -> Option<[u8; 32]> {
    if pk.len() == 32 {
        let mut tmp = [0u8; 32];
        tmp.copy_from_slice(pk);
        Some(tmp)
    } else {
        None
    }
}

pub fn get_rs_pk(str_base64: &str) -> Option<sign::PublicKey> {
    use hbb_common::base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD
        .decode(str_base64.trim())
        .ok()
        .and_then(|pk| get_pk(&pk))
        .map(sign::PublicKey)
}

fn decode_id_pk(
    signed: &[u8],
    key: &sign::PublicKey,
) -> ResultType<(String, [u8; 32], u32)> {
    let res = IdPk::parse_from_bytes(
        &sign::verify(signed, key).map_err(|_| anyhow!("Signature mismatch"))?,
    )?;
    if let Some(pk) = get_pk(&res.pk) {
        Ok((res.id, pk, res.kx_version))
    } else {
        bail!("Wrong their public length");
    }
}

fn create_symmetric_key_msg(
    their_pk_b: [u8; 32],
) -> (hbb_common::bytes::Bytes, hbb_common::bytes::Bytes, secretbox::Key) {
    let their_pk_b = box_::PublicKey(their_pk_b);
    let (our_pk_b, out_sk_b) = box_::gen_keypair();
    let key = secretbox::gen_key();
    let nonce = box_::Nonce([0u8; box_::NONCEBYTES]);
    let sealed_key = box_::seal(&key.0, &nonce, &their_pk_b, &out_sk_b);
    (Vec::from(our_pk_b.0).into(), sealed_key.into(), key)
}

async fn get_next_nonkeyexchange_msg(
    conn: &mut Stream,
    timeout_ms: Option<u64>,
) -> Option<RendezvousMessage> {
    let t = timeout_ms.unwrap_or(READ_TIMEOUT);
    for _ in 0..2 {
        if let Some(Ok(bytes)) = conn.next_timeout(t).await {
            if let Ok(msg_in) = RendezvousMessage::parse_from_bytes(&bytes) {
                match &msg_in.union {
                    Some(rendezvous_message::Union::KeyExchange(_)) => continue,
                    _ => return Some(msg_in),
                }
            }
        }
        break;
    }
    None
}

/// Upstream `key_exchange` with the rendezvous server (used when a token is present).
/// Kept for completeness of the port; the plain-ID flow does not require it.
#[allow(dead_code)]
async fn rendezvous_key_exchange(conn: &mut Stream, key: &str) -> ResultType<bool> {
    let Some(rs_pk) = get_rs_pk(key) else {
        bail!("Handshake failed: invalid public key from rendezvous server");
    };
    match timeout(std::time::Duration::from_millis(READ_TIMEOUT), conn.next()).await? {
        Some(Ok(bytes)) => {
            if let Ok(msg_in) = RendezvousMessage::parse_from_bytes(&bytes) {
                if let Some(rendezvous_message::Union::KeyExchange(ex)) = msg_in.union {
                    if ex.keys.len() != 1 {
                        bail!("Handshake failed: invalid key exchange message");
                    }
                    let their_pk_b = sign::verify(&ex.keys[0], &rs_pk)
                        .map_err(|_| anyhow!("Signature mismatch in key exchange"))?;
                    let their_pk_b =
                        get_pk(&their_pk_b).context("Wrong their public length in key exchange")?;
                    if their_pk_b[31] & 0x80 != 0 || !ex.signed_params.is_empty() {
                        let params = sign::verify(&ex.signed_params, &rs_pk)
                            .ok()
                            .and_then(|signed| {
                                let params =
                                    signed.strip_prefix(hbb_common::tcp::KX_PARAMS_DOMAIN)?;
                                KxParams::parse_from_bytes(params).ok()
                            })
                            .ok_or_else(|| {
                                anyhow!("Missing or invalid signed key exchange parameters")
                            })?;
                        if params.pk[..] != their_pk_b[..] || params.version != ex.version {
                            bail!("Key exchange version or public key does not match its signature");
                        }
                    }
                    let (asymmetric_value, symmetric_value, key) =
                        create_symmetric_key_msg(their_pk_b);
                    let picked = hbb_common::tcp::kx_version_for(ex.version);
                    let mut msg_out = RendezvousMessage::new();
                    msg_out.set_key_exchange(KeyExchange {
                        keys: vec![asymmetric_value.clone(), symmetric_value],
                        version: picked,
                        ..Default::default()
                    });
                    timeout(
                        std::time::Duration::from_millis(CONNECT_TIMEOUT),
                        conn.send(&msg_out),
                    )
                    .await??;
                    conn.set_negotiated_key(
                        key,
                        true,
                        &hbb_common::tcp::KxTranscript {
                            initiator_pk: &asymmetric_value,
                            responder_pk: &their_pk_b,
                            advertised: ex.version,
                            picked,
                        },
                    )?;
                    return Ok(true);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

/// Port of upstream `Client::secure_connection` (non-WebRTC branch).
/// Returns `(peer_pk, secured)`.
async fn secure_connection(
    peer_id: &str,
    signed_id_pk: Vec<u8>,
    key: &str,
    conn: &mut Stream,
) -> ResultType<(Option<Vec<u8>>, bool)> {
    let rs_pk = get_rs_pk(if key.is_empty() {
        hbb_common::config::RS_PUB_KEY
    } else {
        key
    });
    let mut sign_pk = None;
    let mut option_pk = None;
    if !signed_id_pk.is_empty() {
        if let Some(rs_pk) = rs_pk {
            if let Ok((id, pk, _)) = decode_id_pk(&signed_id_pk, &rs_pk) {
                if id == peer_id {
                    sign_pk = Some(sign::PublicKey(pk));
                    option_pk = Some(pk.to_vec());
                }
            }
        }
        if sign_pk.is_none() {
            log::error!("Handshake failed: invalid public key from rendezvous server");
        }
    }
    let Some(sign_pk) = sign_pk else {
        conn.send(&Message::new()).await?;
        return Ok((option_pk, false));
    };
    let mut secured = false;
    match timeout(std::time::Duration::from_millis(READ_TIMEOUT), conn.next()).await? {
        Some(res) => {
            let bytes = res?;
            if let Ok(msg_in) = Message::parse_from_bytes(&bytes) {
                if let Some(message::Union::SignedId(si)) = msg_in.union {
                    if let Ok((id, their_pk_b, kx_version)) = decode_id_pk(&si.id, &sign_pk) {
                        if id == peer_id {
                            let (asymmetric_value, symmetric_value, key) =
                                create_symmetric_key_msg(their_pk_b);
                            let picked = hbb_common::tcp::kx_version_for(kx_version);
                            let mut msg_out = Message::new();
                            msg_out.set_public_key(PublicKey {
                                asymmetric_value: asymmetric_value.clone(),
                                symmetric_value,
                                kx_version: picked,
                                ..Default::default()
                            });
                            timeout(
                                std::time::Duration::from_millis(CONNECT_TIMEOUT),
                                conn.send(&msg_out),
                            )
                            .await??;
                            conn.set_negotiated_key(
                                key,
                                true,
                                &hbb_common::tcp::KxTranscript {
                                    initiator_pk: &asymmetric_value,
                                    responder_pk: &their_pk_b,
                                    advertised: kx_version,
                                    picked,
                                },
                            )?;
                            secured = true;
                        } else {
                            log::error!("Handshake failed: sign failure");
                            conn.send(&Message::new()).await?;
                        }
                    } else {
                        log::info!("pk mismatch, fall back to non-secure");
                        let mut msg_out = Message::new();
                        msg_out.set_public_key(PublicKey::new());
                        conn.send(&msg_out).await?;
                    }
                } else {
                    log::error!("Handshake failed: invalid message type");
                    conn.send(&Message::new()).await?;
                }
            } else {
                log::error!("Handshake failed: invalid message format");
                conn.send(&Message::new()).await?;
            }
        }
        None => bail!("Reset by the peer"),
    }
    Ok((option_pk, secured))
}

async fn create_relay(
    peer: &str,
    uuid: String,
    relay_server: String,
    key: &str,
    ipv4: bool,
) -> ResultType<Stream> {
    let mut conn = connect_tcp(
        ipv4_to_ipv6(check_port(relay_server, RELAY_PORT), ipv4),
        CONNECT_TIMEOUT,
    )
    .await
    .with_context(|| "Failed to connect to relay server")?;
    let mut msg_out = RendezvousMessage::new();
    msg_out.set_request_relay(RequestRelay {
        licence_key: key.to_owned(),
        id: peer.to_owned(),
        uuid,
        conn_type: ConnType::DEFAULT_CONN.into(),
        ..Default::default()
    });
    conn.send(&msg_out).await?;
    Ok(conn)
}

async fn request_relay(
    peer: &str,
    relay_server: String,
    rendezvous_server: &str,
    secure: bool,
    key: &str,
) -> ResultType<Stream> {
    let mut succeed = false;
    let mut uuid = String::new();
    let mut ipv4 = true;
    for i in 1..=3 {
        let mut socket = connect_tcp(rendezvous_server, CONNECT_TIMEOUT)
            .await
            .with_context(|| "Failed to connect to rendezvous server")?;
        ipv4 = socket.local_addr().is_ipv4();
        let mut msg_out = RendezvousMessage::new();
        uuid = hbb_common::uuid::Uuid::new_v4().to_string();
        log::info!("#{i} request relay attempt, uuid: {uuid}, relay_server: {relay_server}");
        msg_out.set_request_relay(RequestRelay {
            id: peer.to_owned(),
            uuid: uuid.clone(),
            relay_server: relay_server.clone(),
            secure,
            ..Default::default()
        });
        socket.send(&msg_out).await?;
        if let Some(msg_in) = get_next_nonkeyexchange_msg(&mut socket, Some(CONNECT_TIMEOUT)).await
        {
            if let Some(rendezvous_message::Union::RelayResponse(rs)) = msg_in.union {
                if !rs.refuse_reason.is_empty() {
                    bail!(rs.refuse_reason);
                }
                succeed = true;
                break;
            }
        }
    }
    if !succeed {
        bail!("Timeout");
    }
    create_relay(peer, uuid, relay_server, key, ipv4).await
}

/// Establish a secured stream to `target` using `profile`.
pub async fn establish(
    profile: &ServerProfile,
    target: &Target,
    force_relay: bool,
) -> ResultType<Established> {
    let key = profile.effective_key();
    match target {
        Target::Address(addr) => {
            // Upstream: `connect_tcp_local(check_port(peer, RELAY_PORT + 1), ...)` for IPs;
            // host:port is used verbatim.
            let target = if hbb_common::is_ip_str(addr) {
                check_port(addr, RELAY_PORT + 1)
            } else {
                addr.clone()
            };
            let mut stream = connect_tcp_local(target, None, CONNECT_TIMEOUT).await?;
            // No rendezvous-signed key exists for direct addresses: upstream sends an empty
            // message and proceeds unencrypted-at-handshake (the host decides whether direct
            // access is permitted at all).
            let (peer_pk, secured) =
                secure_connection(addr, Vec::new(), &key, &mut stream).await?;
            Ok(Established {
                stream,
                route: Route::DirectAddress,
                secured,
                peer_pk,
            })
        }
        Target::Id(peer) => establish_by_id(profile, peer, &key, force_relay).await,
    }
}

async fn establish_by_id(
    profile: &ServerProfile,
    peer: &str,
    key: &str,
    force_relay: bool,
) -> ResultType<Established> {
    let servers = profile.rendezvous_servers();
    if servers.is_empty() {
        bail!("No ID server configured");
    }
    let mut start = Instant::now();
    let mut rendezvous_server = check_port(&servers[0], RENDEZVOUS_PORT);
    let mut socket = connect_tcp(&*rendezvous_server, CONNECT_TIMEOUT).await;
    if socket.is_err() {
        for s in servers.iter().skip(1) {
            let s = check_port(s, RENDEZVOUS_PORT);
            socket = connect_tcp(&*s, CONNECT_TIMEOUT).await;
            if socket.is_ok() {
                rendezvous_server = s;
                break;
            }
        }
    }
    let mut socket = socket.map_err(|e| anyhow!("Failed to connect to ID server: {e}"))?;
    let rtt = start.elapsed();
    log::info!("rendezvous server reached in {:?}", rtt);
    let my_addr = socket.local_addr();
    let nat_type = if force_relay {
        NatType::SYMMETRIC
    } else {
        NatType::UNKNOWN_NAT
    };
    let mut msg_out = RendezvousMessage::new();
    msg_out.set_punch_hole_request(PunchHoleRequest {
        id: peer.to_owned(),
        nat_type: nat_type.into(),
        licence_key: key.to_owned(),
        conn_type: ConnType::DEFAULT_CONN.into(),
        version: PROTOCOL_VERSION.to_owned(),
        force_relay,
        ..Default::default()
    });
    let mut signed_id_pk = Vec::new();
    let mut relay_server = String::new();
    let mut peer_addr: Option<SocketAddr> = None;
    let mut peer_nat_type = NatType::UNKNOWN_NAT;
    let mut is_local = false;
    'punch: for i in 1..=3u64 {
        log::info!("#{i} TCP punch attempt with {my_addr}");
        socket.send(&msg_out).await?;
        let deadline = Instant::now() + std::time::Duration::from_millis(i * 3000);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let Some(msg_in) =
                get_next_nonkeyexchange_msg(&mut socket, Some(remaining.as_millis().max(1) as u64))
                    .await
            else {
                break;
            };
            match msg_in.union {
                Some(rendezvous_message::Union::PunchHoleResponse(ph)) => {
                    if ph.socket_addr.is_empty() {
                        if !ph.other_failure.is_empty() {
                            bail!(ph.other_failure);
                        }
                        match ph.failure.enum_value() {
                            Ok(punch_hole_response::Failure::ID_NOT_EXIST) => {
                                bail!("ID does not exist")
                            }
                            Ok(punch_hole_response::Failure::OFFLINE) => {
                                bail!("Remote desktop is offline")
                            }
                            Ok(punch_hole_response::Failure::LICENSE_MISMATCH) => {
                                bail!("Key mismatch")
                            }
                            Ok(punch_hole_response::Failure::LICENSE_OVERUSE) => {
                                bail!("Key overuse")
                            }
                            _ => bail!("other punch hole failure"),
                        }
                    }
                    peer_nat_type = ph.nat_type();
                    is_local = ph.is_local();
                    signed_id_pk = ph.pk.into();
                    relay_server = ph.relay_server;
                    peer_addr = Some(AddrMangle::decode(&ph.socket_addr));
                    log::info!("Hole punched, peer nat {:?}, local {}", peer_nat_type, is_local);
                    break 'punch;
                }
                Some(rendezvous_message::Union::RelayResponse(rr)) => {
                    log::info!("relay requested from peer, relay_server: {}", rr.relay_server);
                    start = Instant::now();
                    signed_id_pk = rr.pk().into();
                    let mut conn =
                        create_relay(peer, rr.uuid, rr.relay_server, key, my_addr.is_ipv4())
                            .await?;
                    log::info!("{:?} used to establish relay connection", start.elapsed());
                    let (peer_pk, secured) =
                        secure_connection(peer, signed_id_pk, key, &mut conn).await?;
                    return Ok(Established {
                        stream: conn,
                        route: Route::Relay,
                        secured,
                        peer_pk,
                    });
                }
                other => log::warn!("Unexpected rendezvous message: {:?}", other.is_some()),
            }
        }
    }
    drop(socket);
    let Some(peer_addr) = peer_addr.filter(|a| a.port() != 0) else {
        bail!("Failed to connect via rendezvous server");
    };
    let punch_time_used = start.elapsed().as_millis() as u64;
    // Upstream `connect` timeout selection, simplified for an unknown local NAT type.
    const MIN: u64 = 1000;
    let connect_timeout = if is_local || peer_nat_type == NatType::SYMMETRIC {
        MIN
    } else if relay_server.is_empty() {
        CONNECT_TIMEOUT
    } else {
        (punch_time_used * 6).max(MIN)
    };
    let mut direct = None;
    if !force_relay {
        match connect_tcp_local(peer_addr, Some(my_addr), connect_timeout).await {
            Ok(c) => direct = Some(c),
            Err(e) => log::info!("direct TCP connect failed: {e}"),
        }
    }
    let (mut conn, route) = match direct {
        Some(c) => (c, Route::DirectPunch),
        None => {
            if relay_server.is_empty() {
                bail!("Failed to make direct connection to remote desktop");
            }
            let c = request_relay(
                peer,
                relay_server,
                &rendezvous_server,
                !signed_id_pk.is_empty(),
                key,
            )
            .await
            .map_err(|e| anyhow!("Failed to connect via relay server: {e}"))?;
            (c, Route::Relay)
        }
    };
    let (peer_pk, secured) = secure_connection(peer, signed_id_pk, key, &mut conn).await?;
    Ok(Established {
        stream: conn,
        route,
        secured,
        peer_pk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parsing() {
        assert_eq!(Target::parse(" 123 456 789 "), Some(Target::Id("123456789".into())));
        assert_eq!(
            Target::parse("192.168.1.20"),
            Some(Target::Address("192.168.1.20".into()))
        );
        assert_eq!(
            Target::parse("pc.example.com:21118"),
            Some(Target::Address("pc.example.com:21118".into()))
        );
        assert_eq!(Target::parse(""), None);
        assert_eq!(Target::parse("bad id!"), None);
    }

    #[test]
    fn public_key_decoding() {
        assert!(get_rs_pk(hbb_common::config::RS_PUB_KEY).is_some());
        assert!(get_rs_pk("not-base64").is_none());
        assert!(get_rs_pk("AAAA").is_none());
    }
}

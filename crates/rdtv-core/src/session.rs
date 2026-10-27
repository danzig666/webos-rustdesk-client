//! One outbound remote-desktop session: login, message loop, typed events/commands.
//!
//! Login and message handling are ported from RustDesk 1.5.0 (`src/client.rs`:
//! `handle_hash`, `create_login_msg`, `get_option_message`, `handle_login_error`,
//! `handle_test_delay`; `src/client/io_loop.rs`: `handle_msg_from_peer`, the 30 s receive
//! timeout). Copyright (c) RustDesk contributors, AGPL-3.0.
//!
//! Excluded by owner decision and therefore never requested or honoured: clipboard sync,
//! file transfer, chat, recording, screenshots, terminal, port forwarding, voice calls.
//! The login option message disables clipboard and file transfer explicitly, and incoming
//! clipboard/file/chat messages are dropped without side effects.

use crate::{
    profile::ServerProfile,
    reconnect::{classify, FailureKind},
    store::{CodecPreference, PeerSettings, Quality},
    transport::{self, Route, Target},
};
use base::message_proto::*;
use hbb_common::{
    protobuf::{Message as _, MessageField},
    sha2::{Digest, Sha256},
    tokio::{
        self,
        sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        time::{interval, Duration, Instant},
    },
    Stream,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

pub const LOGIN_MSG_PASSWORD_EMPTY: &str = "Empty Password";
pub const LOGIN_MSG_PASSWORD_WRONG: &str = "Wrong Password";
pub const LOGIN_MSG_2FA_WRONG: &str = "Wrong 2FA Code";
pub const REQUIRE_2FA: &str = "2FA Required";
pub const LOGIN_MSG_NO_PASSWORD_ACCESS: &str = "No Password Access";
const RECV_TIMEOUT: Duration = Duration::from_secs(30);

/// Codecs this TV can decode, as reported by the media layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeCaps {
    pub vp8: bool,
    pub vp9: bool,
    pub h264: bool,
    pub h265: bool,
    pub av1: bool,
}

impl DecodeCaps {
    pub fn to_supported_decoding(self, pref: CodecPreference) -> SupportedDecoding {
        use supported_decoding::PreferCodec;
        let prefer = match pref {
            CodecPreference::Auto => PreferCodec::Auto,
            CodecPreference::H265 if self.h265 => PreferCodec::H265,
            CodecPreference::H264 if self.h264 => PreferCodec::H264,
            CodecPreference::Vp9 if self.vp9 => PreferCodec::VP9,
            CodecPreference::Vp8 if self.vp8 => PreferCodec::VP8,
            _ => PreferCodec::Auto,
        };
        SupportedDecoding {
            ability_vp8: self.vp8 as i32,
            ability_vp9: self.vp9 as i32,
            ability_h264: self.h264 as i32,
            ability_h265: self.h265 as i32,
            ability_av1: self.av1 as i32,
            prefer: prefer.into(),
            ..Default::default()
        }
    }
}

/// Media consumer. Called on the network runtime: implementations must not block.
pub trait MediaSink: Send + Sync {
    fn video(&self, generation: u64, frame: VideoFrame);
    fn audio_format(&self, generation: u64, format: AudioFormat);
    fn audio(&self, generation: u64, frame: AudioFrame);
}

#[derive(Debug, Clone)]
pub struct SessionRequest {
    pub profile: ServerProfile,
    pub target: Target,
    /// Password typed for this attempt (plaintext only in memory, never persisted).
    pub password: Option<String>,
    /// Remembered salted hash `sha256(password || salt)` for this profile+peer.
    pub remembered_hash: Option<Vec<u8>>,
    /// Ask to remember the typed password once login succeeds.
    pub remember: bool,
    pub settings: PeerSettings,
    pub caps: DecodeCaps,
    pub audio_enabled: bool,
    pub client_id: String,
    pub client_name: String,
    pub force_relay: bool,
    /// Account access token for this profile ("" when signed out), sent to the ID server.
    pub access_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    ConnectingServer,
    Connecting,
    LoggingIn,
    WaitingApproval,
    Connected,
}

#[derive(Debug, Clone)]
pub struct DisplayDesc {
    pub index: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub name: String,
    pub online: bool,
    pub cursor_embedded: bool,
    pub scale: f64,
}

impl From<(usize, &DisplayInfo)> for DisplayDesc {
    fn from((i, d): (usize, &DisplayInfo)) -> Self {
        DisplayDesc {
            index: i as i32,
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            name: d.name.clone(),
            online: d.online,
            cursor_embedded: d.cursor_embedded,
            scale: d.scale,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeerSummary {
    pub username: String,
    pub hostname: String,
    pub platform: String,
    pub version: String,
    pub displays: Vec<DisplayDesc>,
    pub current_display: i32,
    pub sas_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct CursorImage {
    pub id: u64,
    pub hotx: i32,
    pub hoty: i32,
    pub width: i32,
    pub height: i32,
    /// RGBA8, `width*height*4` bytes (validated).
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    Phase(Phase),
    /// The host wants a password. `wrong` = previous one was rejected.
    PasswordRequired {
        wrong: bool,
    },
    TwoFactorRequired {
        wrong: bool,
    },
    Connected {
        peer: PeerSummary,
        route: Route,
        secured: bool,
        /// Salted hash that logged in, kept in memory only so an automatic reconnect can
        /// log in again (persisted only through `RememberHash` after the user opted in).
        session_hash: Option<Vec<u8>>,
    },
    /// Store this salted hash for the profile+peer (user opted in).
    RememberHash(Vec<u8>),
    /// The remembered hash was rejected; forget it.
    ForgetHash,
    Displays(Vec<DisplayDesc>),
    SwitchDisplay(DisplayDesc),
    Cursor(CursorImage),
    CursorId(u64),
    CursorPosition {
        x: i32,
        y: i32,
    },
    Permission {
        name: String,
        enabled: bool,
    },
    /// Informational host message (msgbox), already sanitized.
    Notice {
        title: String,
        text: String,
    },
    /// Host reported the remote side is in a UAC/elevated state.
    Uac(bool),
    Ended(EndReason),
}

#[derive(Debug, Clone)]
pub struct EndReason {
    pub message: String,
    pub kind: FailureKind,
    /// True for user-requested disconnects: never auto-retried.
    pub intentional: bool,
    /// True when the session had reached `Connected` (eligible for reconnect).
    pub was_connected: bool,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SessionCommand {
    Send(Message),
    SendMany(Vec<Message>),
    Password {
        password: String,
        remember: bool,
    },
    TwoFactor {
        code: String,
    },
    SwitchDisplay(i32),
    RefreshVideo(i32),
    SetQuality(Quality),
    SetFps(u32),
    SetAudio(bool),
    ChangeResolution {
        display: i32,
        width: i32,
        height: i32,
    },
    /// Re-advertise decodable codecs (e.g. after a hardware decoder failed); upstream
    /// `update_supported_decodings`.
    UpdateDecoding(DecodeCaps),
    /// End the session. `lock` sends upstream's LockScreen first when the transport is up.
    Disconnect {
        lock: bool,
    },
}

#[derive(Clone)]
pub struct SessionHandle {
    pub generation: u64,
    tx: UnboundedSender<SessionCommand>,
}

impl SessionHandle {
    pub fn send(&self, cmd: SessionCommand) -> bool {
        self.tx.send(cmd).is_ok()
    }
    pub fn send_msg(&self, msg: Message) -> bool {
        self.send(SessionCommand::Send(msg))
    }
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Start a session on `rt`. Events are delivered through `on_event` tagged with the
/// session generation so the UI can drop events from stale sessions.
pub fn start(
    rt: &tokio::runtime::Handle,
    req: SessionRequest,
    media: Arc<dyn MediaSink>,
    on_event: Arc<dyn Fn(u64, SessionEvent) + Send + Sync>,
) -> SessionHandle {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = unbounded_channel();
    rt.spawn(async move {
        let emit = |e: SessionEvent| on_event(generation, e);
        let end = run(generation, req, media, rx, &emit).await;
        emit(SessionEvent::Ended(end));
    });
    SessionHandle { generation, tx }
}

fn quality_enum(q: Quality) -> ImageQuality {
    match q {
        Quality::Balanced => ImageQuality::Balanced,
        Quality::Best => ImageQuality::Best,
        Quality::Low => ImageQuality::Low,
    }
}

fn bool_opt(b: bool) -> hbb_common::protobuf::EnumOrUnknown<option_message::BoolOption> {
    if b {
        option_message::BoolOption::Yes.into()
    } else {
        option_message::BoolOption::No.into()
    }
}

/// Port of upstream `get_option_message` restricted to this product's scope.
fn option_message(req: &SessionRequest) -> OptionMessage {
    let s = &req.settings;
    let mut msg = OptionMessage::new();
    msg.image_quality = quality_enum(s.quality).into();
    if s.fps > 0 {
        msg.custom_fps = s.fps as i32;
    }
    if s.show_remote_cursor {
        msg.show_remote_cursor = bool_opt(true);
    }
    if s.lock_on_disconnect {
        msg.lock_after_session_end = bool_opt(true);
    }
    if !req.audio_enabled {
        msg.disable_audio = bool_opt(true);
    }
    // Owner exclusions: never enable clipboard or file transfer.
    msg.disable_clipboard = bool_opt(true);
    msg.enable_file_transfer = bool_opt(false);
    msg.supported_decoding = MessageField::some(req.caps.to_supported_decoding(s.codec));
    msg
}

fn login_msg(req: &SessionRequest, password: Vec<u8>, session_id: u64) -> Message {
    let lr = LoginRequest {
        username: req.target.as_str().to_owned(),
        password: password.into(),
        my_id: req.client_id.clone(),
        my_name: req.client_name.clone(),
        my_platform: "Linux".to_owned(),
        option: MessageField::some(option_message(req)),
        session_id,
        version: transport::PROTOCOL_VERSION.to_owned(),
        ..Default::default()
    };
    let mut msg = Message::new();
    msg.set_login_request(lr);
    msg
}

fn salted(password: &str, salt: &str) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(password);
    h.update(salt);
    h.finalize()[..].into()
}

fn challenge_response(salted: &[u8], challenge: &str) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(salted);
    h.update(challenge);
    h.finalize()[..].into()
}

fn misc_msg(f: impl FnOnce(&mut Misc)) -> Message {
    let mut misc = Misc::new();
    f(&mut misc);
    let mut msg = Message::new();
    msg.set_misc(misc);
    msg
}

pub fn refresh_msg(display: i32, peer_version: i64) -> Message {
    // Upstream uses refresh_video_display for peers that support multiple UI sessions
    // (>= 1.2.4) and the older refresh_video otherwise.
    if peer_version >= hbb_common::get_version_number("1.2.4") {
        misc_msg(|m| m.set_refresh_video_display(display))
    } else {
        misc_msg(|m| m.set_refresh_video(true))
    }
}

fn switch_display_msgs(display: i32) -> Vec<Message> {
    vec![
        misc_msg(|m| {
            m.set_switch_display(SwitchDisplay {
                display,
                ..Default::default()
            })
        }),
        misc_msg(|m| {
            m.set_capture_displays(CaptureDisplays {
                set: vec![display],
                ..Default::default()
            })
        }),
    ]
}

/// Maximum cursor size accepted from a host (defends against malformed CursorData).
const MAX_CURSOR_DIM: i32 = 256;

fn cursor_from(cd: CursorData) -> Option<CursorImage> {
    if cd.width <= 0 || cd.height <= 0 || cd.width > MAX_CURSOR_DIM || cd.height > MAX_CURSOR_DIM {
        return None;
    }
    let need = (cd.width * cd.height * 4) as usize;
    let mut rgba = cd.colors.to_vec();
    // Upstream compresses cursor colors with zstd when they are not raw RGBA.
    if rgba.len() != need {
        rgba = hbb_common::compress::decompress(&rgba);
    }
    if rgba.len() != need {
        return None;
    }
    Some(CursorImage {
        id: cd.id,
        hotx: cd.hotx.clamp(0, cd.width - 1),
        hoty: cd.hoty.clamp(0, cd.height - 1),
        width: cd.width,
        height: cd.height,
        rgba,
    })
}

struct LoginState {
    salt: String,
    challenge: String,
    /// Salted hash we last sent and where it came from.
    sent: Option<(Vec<u8>, bool /* from remembered */)>,
    remember_pending: bool,
}

async fn run(
    generation: u64,
    mut req: SessionRequest,
    media: Arc<dyn MediaSink>,
    mut rx: UnboundedReceiver<SessionCommand>,
    emit: &(dyn Fn(SessionEvent) + Send + Sync),
) -> EndReason {
    let fail = |msg: String, was_connected: bool| EndReason {
        kind: classify(&msg),
        message: msg,
        intentional: false,
        was_connected,
    };
    emit(SessionEvent::Phase(match req.target {
        Target::Id(_) => Phase::ConnectingServer,
        Target::Address(_) => Phase::Connecting,
    }));
    // Establishment can be cancelled by a Disconnect command.
    let established = {
        let fut = transport::establish(&req.profile, &req.target, req.force_relay, &req.access_token);
        tokio::pin!(fut);
        loop {
            tokio::select! {
                r = &mut fut => break r,
                cmd = rx.recv() => match cmd {
                    Some(SessionCommand::Disconnect { .. }) | None => {
                        return EndReason {
                            message: "Cancelled".into(),
                            kind: FailureKind::Other,
                            intentional: true,
                            was_connected: false,
                        };
                    }
                    _ => {}
                }
            }
        }
    };
    let est = match established {
        Ok(e) => e,
        Err(e) => return fail(e.to_string(), false),
    };
    let route = est.route;
    let secured = est.secured;
    let mut peer: Stream = est.stream;
    emit(SessionEvent::Phase(Phase::LoggingIn));
    log::info!(
        "session {generation}: transport up ({}), secured={secured}",
        route.label()
    );

    let session_id = hbb_common::rand::random::<u64>();
    let mut login = LoginState {
        salt: String::new(),
        challenge: String::new(),
        sent: None,
        remember_pending: false,
    };
    let mut connected = false;
    let mut peer_version: i64 = 0;
    let mut last_recv = Instant::now();
    let mut timer = interval(Duration::from_secs(1));
    let mut displays: Vec<DisplayDesc> = Vec::new();

    let end = loop {
        tokio::select! {
            res = peer.next() => {
                let bytes = match res {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => break fail(e.to_string(), connected),
                    None => break fail("Reset by the peer".into(), connected),
                };
                last_recv = Instant::now();
                let msg_in = match Message::parse_from_bytes(&bytes) {
                    Ok(m) => m,
                    Err(_) => {
                        log::trace!("dropping unparsable message");
                        continue;
                    }
                };
                match msg_in.union {
                    Some(message::Union::VideoFrame(vf)) => {
                        if connected {
                            media.video(generation, vf);
                        }
                    }
                    Some(message::Union::Hash(hash)) => {
                        login.salt = hash.salt.clone();
                        login.challenge = hash.challenge.clone();
                        // Upstream order: typed/last password, then remembered config password.
                        let salted_hash = if let Some(p) = req.password.as_ref().filter(|p| !p.is_empty()) {
                            login.remember_pending = req.remember;
                            Some((salted(p, &hash.salt), false))
                        } else {
                            req.remembered_hash.clone().map(|h| (h, true))
                        };
                        let pw = match &salted_hash {
                            Some((s, _)) => challenge_response(s, &hash.challenge),
                            None => {
                                // Login without password: the host may let its user accept.
                                emit(SessionEvent::PasswordRequired { wrong: false });
                                emit(SessionEvent::Phase(Phase::WaitingApproval));
                                Vec::new()
                            }
                        };
                        login.sent = salted_hash;
                        if let Err(e) = peer.send(&login_msg(&req, pw, session_id)).await {
                            break fail(e.to_string(), false);
                        }
                    }
                    Some(message::Union::LoginResponse(lr)) => match lr.union {
                        Some(login_response::Union::Error(err)) => {
                            log::info!("login error: {err}");
                            if err == LOGIN_MSG_PASSWORD_EMPTY {
                                emit(SessionEvent::PasswordRequired { wrong: false });
                            } else if err == LOGIN_MSG_PASSWORD_WRONG {
                                if matches!(login.sent, Some((_, true))) {
                                    req.remembered_hash = None;
                                    emit(SessionEvent::ForgetHash);
                                }
                                req.password = None;
                                login.sent = None;
                                emit(SessionEvent::PasswordRequired { wrong: true });
                            } else if err == REQUIRE_2FA || err == LOGIN_MSG_2FA_WRONG {
                                emit(SessionEvent::TwoFactorRequired { wrong: err == LOGIN_MSG_2FA_WRONG });
                            } else if err == LOGIN_MSG_NO_PASSWORD_ACCESS {
                                emit(SessionEvent::Phase(Phase::WaitingApproval));
                            } else {
                                break fail(err, false);
                            }
                        }
                        Some(login_response::Union::PeerInfo(pi)) => {
                            peer_version = hbb_common::get_version_number(&pi.version);
                            displays = pi.displays.iter().enumerate().map(DisplayDesc::from).collect();
                            let summary = PeerSummary {
                                username: pi.username.clone(),
                                hostname: pi.hostname.clone(),
                                platform: pi.platform.clone(),
                                version: pi.version.clone(),
                                displays: displays.clone(),
                                current_display: pi.current_display,
                                sas_enabled: pi.sas_enabled,
                            };
                            if login.remember_pending {
                                if let Some((s, false)) = &login.sent {
                                    emit(SessionEvent::RememberHash(s.clone()));
                                }
                            }
                            connected = true;
                            let session_hash = login.sent.as_ref().map(|(s, _)| s.clone());
                            emit(SessionEvent::Connected { peer: summary, route, secured, session_hash });
                            emit(SessionEvent::Phase(Phase::Connected));
                            // Restore the saved monitor when it still exists (visible fallback
                            // to the host's current display otherwise).
                            let want = req.settings.monitor;
                            if want != pi.current_display && want >= 0 && (want as usize) < displays.len() {
                                for m in switch_display_msgs(want) {
                                    let _ = peer.send(&m).await;
                                }
                            }
                        }
                        _ => {}
                    },
                    Some(message::Union::TestDelay(t)) => {
                        // Must be echoed or the host's latency control misbehaves.
                        if !t.from_client {
                            let mut m = Message::new();
                            m.set_test_delay(t);
                            let _ = peer.send(&m).await;
                        }
                    }
                    Some(message::Union::AudioFrame(f)) => {
                        if connected {
                            media.audio(generation, f);
                        }
                    }
                    Some(message::Union::CursorData(cd)) => {
                        if let Some(c) = cursor_from(cd) {
                            emit(SessionEvent::Cursor(c));
                        }
                    }
                    Some(message::Union::CursorId(id)) => emit(SessionEvent::CursorId(id)),
                    Some(message::Union::CursorPosition(cp)) => {
                        emit(SessionEvent::CursorPosition { x: cp.x, y: cp.y })
                    }
                    Some(message::Union::PeerInfo(pi)) => {
                        displays = pi.displays.iter().enumerate().map(DisplayDesc::from).collect();
                        emit(SessionEvent::Displays(displays.clone()));
                    }
                    Some(message::Union::MessageBox(mb)) => {
                        // Upstream drops links not in its allow-list; we show no links at all.
                        emit(SessionEvent::Notice { title: mb.title, text: mb.text });
                    }
                    Some(message::Union::Misc(misc)) => match misc.union {
                        Some(misc::Union::AudioFormat(f)) => media.audio_format(generation, f),
                        Some(misc::Union::SwitchDisplay(s)) => {
                            let scale = displays
                                .get(s.display as usize)
                                .map(|d| d.scale)
                                .unwrap_or(1.0);
                            let name = displays
                                .get(s.display as usize)
                                .map(|d| d.name.clone())
                                .unwrap_or_default();
                            emit(SessionEvent::SwitchDisplay(DisplayDesc {
                                index: s.display,
                                x: s.x,
                                y: s.y,
                                width: s.width,
                                height: s.height,
                                name,
                                online: true,
                                cursor_embedded: s.cursor_embedded,
                                scale,
                            }));
                        }
                        Some(misc::Union::PermissionInfo(p)) => {
                            let name = format!("{:?}", p.permission.enum_value_or_default());
                            emit(SessionEvent::Permission { name, enabled: p.enabled });
                        }
                        Some(misc::Union::CloseReason(c)) => break fail(c, connected),
                        Some(misc::Union::Uac(u)) => emit(SessionEvent::Uac(u)),
                        Some(misc::Union::ForegroundWindowElevated(e)) => emit(SessionEvent::Uac(e)),
                        // Chat, clipboard and file-transfer related misc are out of scope.
                        _ => {}
                    },
                    // Clipboard / file / chat / voice / terminal / screenshot messages are
                    // excluded by owner decision: ignore without side effects.
                    _ => {}
                }
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else {
                    break EndReason { message: "Closed".into(), kind: FailureKind::Other, intentional: true, was_connected: connected };
                };
                let r = match cmd {
                    SessionCommand::Send(m) => {
                        if connected { peer.send(&m).await } else { Ok(()) }
                    }
                    SessionCommand::SendMany(ms) => {
                        let mut r = Ok(());
                        if connected {
                            for m in ms {
                                r = peer.send(&m).await;
                                if r.is_err() { break; }
                            }
                        }
                        r
                    }
                    SessionCommand::Password { password, remember } => {
                        if login.salt.is_empty() {
                            req.password = Some(password);
                            req.remember = remember;
                            Ok(())
                        } else {
                            let s = salted(&password, &login.salt);
                            let pw = challenge_response(&s, &login.challenge);
                            login.sent = Some((s, false));
                            login.remember_pending = remember;
                            emit(SessionEvent::Phase(Phase::LoggingIn));
                            peer.send(&login_msg(&req, pw, session_id)).await
                        }
                    }
                    SessionCommand::TwoFactor { code } => {
                        let mut m = Message::new();
                        m.set_auth_2fa(Auth2FA { code, ..Default::default() });
                        peer.send(&m).await
                    }
                    SessionCommand::SwitchDisplay(d) => {
                        let mut r = Ok(());
                        if connected {
                            for m in switch_display_msgs(d) {
                                r = peer.send(&m).await;
                            }
                        }
                        r
                    }
                    SessionCommand::RefreshVideo(d) => {
                        if connected { peer.send(&refresh_msg(d, peer_version)).await } else { Ok(()) }
                    }
                    SessionCommand::SetQuality(q) => {
                        req.settings.quality = q;
                        let m = misc_msg(|m| m.set_option(OptionMessage {
                            image_quality: quality_enum(q).into(),
                            ..Default::default()
                        }));
                        if connected { peer.send(&m).await } else { Ok(()) }
                    }
                    SessionCommand::SetFps(fps) => {
                        req.settings.fps = fps;
                        let m = misc_msg(|m| m.set_option(OptionMessage {
                            custom_fps: fps as i32,
                            ..Default::default()
                        }));
                        if connected && fps > 0 { peer.send(&m).await } else { Ok(()) }
                    }
                    SessionCommand::SetAudio(on) => {
                        req.audio_enabled = on;
                        let m = misc_msg(|m| m.set_option(OptionMessage {
                            disable_audio: bool_opt(!on),
                            ..Default::default()
                        }));
                        if connected { peer.send(&m).await } else { Ok(()) }
                    }
                    SessionCommand::ChangeResolution { display, width, height } => {
                        let m = misc_msg(|m| m.set_change_display_resolution(DisplayResolution {
                            display,
                            resolution: MessageField::some(Resolution { width, height, ..Default::default() }),
                            ..Default::default()
                        }));
                        if connected { peer.send(&m).await } else { Ok(()) }
                    }
                    SessionCommand::UpdateDecoding(caps) => {
                        req.caps = caps;
                        let m = misc_msg(|m| m.set_option(OptionMessage {
                            supported_decoding: MessageField::some(caps.to_supported_decoding(req.settings.codec)),
                            ..Default::default()
                        }));
                        if connected { peer.send(&m).await } else { Ok(()) }
                    }
                    SessionCommand::Disconnect { lock } => {
                        if connected && lock {
                            let _ = peer.send(&crate::input::lock_screen()).await;
                        }
                        // Upstream sends a close reason so the host shows why the session ended.
                        let m = misc_msg(|m| m.set_close_reason("Closed manually by the peer".into()));
                        let _ = peer.send(&m).await;
                        break EndReason { message: "Disconnected".into(), kind: FailureKind::Other, intentional: true, was_connected: connected };
                    }
                };
                if let Err(e) = r {
                    break fail(e.to_string(), connected);
                }
            }
            _ = timer.tick() => {
                if last_recv.elapsed() >= RECV_TIMEOUT {
                    break fail("Connection lost: no data for 30 seconds (Timeout)".into(), connected);
                }
            }
        }
    };
    // A lost transport needs no further sends; just report.
    if !end.intentional && end.was_connected && end.kind == FailureKind::Other {
        return EndReason {
            kind: FailureKind::ConnectionLost,
            ..end
        };
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> SessionRequest {
        SessionRequest {
            profile: ServerProfile::public(),
            target: Target::Id("123456789".into()),
            password: None,
            remembered_hash: None,
            remember: false,
            settings: PeerSettings::default(),
            caps: DecodeCaps {
                vp9: true,
                h264: true,
                h265: true,
                ..Default::default()
            },
            audio_enabled: true,
            client_id: "111222333".into(),
            client_name: "LG TV".into(),
            force_relay: false,
            access_token: String::new(),
        }
    }

    #[test]
    fn login_message_disables_excluded_features() {
        let m = login_msg(&req(), vec![], 1);
        let lr = m.login_request();
        let o = lr.option.as_ref().unwrap();
        assert_eq!(
            o.disable_clipboard.enum_value_or_default(),
            option_message::BoolOption::Yes
        );
        assert_eq!(
            o.enable_file_transfer.enum_value_or_default(),
            option_message::BoolOption::No
        );
        let d = o.supported_decoding.as_ref().unwrap();
        assert_eq!(
            (d.ability_vp9, d.ability_h264, d.ability_h265, d.ability_av1),
            (1, 1, 1, 0)
        );
        assert_eq!(lr.username, "123456789");
        assert!(lr.file_transfer().dir.is_empty());
    }

    #[test]
    fn password_hash_matches_upstream_scheme() {
        // sha256(sha256(pw || salt) || challenge)
        let s = salted("secret", "SALT");
        let mut h = Sha256::new();
        h.update(b"secret");
        h.update(b"SALT");
        assert_eq!(s, h.finalize().to_vec());
        let r = challenge_response(&s, "CH");
        assert_eq!(r.len(), 32);
        assert_ne!(r, s);
    }

    #[test]
    fn codec_preference_falls_back_when_unsupported() {
        let caps = DecodeCaps {
            vp9: true,
            ..Default::default()
        };
        let d = caps.to_supported_decoding(CodecPreference::H265);
        assert_eq!(
            d.prefer.enum_value_or_default(),
            supported_decoding::PreferCodec::Auto
        );
    }

    #[test]
    fn malformed_cursor_rejected() {
        let mut cd = CursorData::new();
        cd.width = 10000;
        cd.height = 1;
        assert!(cursor_from(cd).is_none());
        let mut cd = CursorData::new();
        cd.width = 2;
        cd.height = 2;
        cd.colors = vec![0u8; 16].into();
        assert!(cursor_from(cd).is_some());
        let mut cd = CursorData::new();
        cd.width = 2;
        cd.height = 2;
        cd.colors = vec![0u8; 3].into();
        assert!(cursor_from(cd).is_none());
    }
}

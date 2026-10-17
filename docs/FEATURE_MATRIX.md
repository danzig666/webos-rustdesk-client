# Feature matrix

Completeness is measured against the **owner-approved scope** (brief section 13), not every desktop
RustDesk feature.

Status values: `planned`, `implemented but unverified`, `verified`, `platform blocked with evidence`,
`server/host capability unavailable`, `not supported upstream`, `excluded by owner`.

"Verified" below means **verified against a stock RustDesk 1.5.0 Linux host in the development
container** (desktop simulator build, or the ARM build under qemu). Nothing is verified on a TV
yet. Every TV-dependent item is "implemented but unverified" until the
[checklist](NONROOT_CHECKLIST.md) results come in. Evidence:
[docs/evidence/2026-10-17-linux-host-probe.md](evidence/2026-10-17-linux-host-probe.md).

| Feature family | Upstream feature / API (1.5.0) | Host / server prerequisite | Implementation (source) | TV prerequisite | Dev Mode / rooted | UI entry point | Status | Test / evidence |
|---|---|---|---|---|---|---|---|---|
| Connections: direct address | `connect_tcp_local(ip:21118)` (`src/client.rs` `_start`) | Host option "Enable direct IP access" | `rdtv-core/src/transport.rs` `establish` | network | both, no privilege | Home ID field | **verified** (desktop + ARM/qemu) | probe evidence; `transport::tests::target_parsing` |
| Connections: ID via rendezvous, TCP punch | `PunchHoleRequest`/`PunchHoleResponse`, `connect` | Host registered on the server | `establish_by_id` | outbound 21116/21117 | both | Home ID field | implemented but unverified | needs a host reachable through a rendezvous server (container blocks 21116) |
| Connections: relay | `RequestRelay`, `create_relay`, `request_relay` | Relay server (hbbr) | `create_relay`, `request_relay` | outbound 21117 | both | automatic | implemented but unverified | same as above |
| Connections: WebRTC / UDP-KCP / IPv6 punch | `WebRTCStream`, `udp_nat_connect` | — | not implemented (decision D4) | — | — | — | not required (upstream fallbacks used) | — |
| Cancellation, errors, bounded reconnect | `io_loop` 30 s timeout | — | `session.rs`, `reconnect.rs`, `app.rs` | — | both | Connecting/Reconnecting dialogs | implemented; policy unit-tested | `reconnect::tests`; manual cancel tested in simulator |
| Server profiles: public + self-hosted, key verification | `RENDEZVOUS_SERVERS`, `RS_PUB_KEY`, `secure_connection`, `get_api_server_` | — | `profile.rs`, `transport::secure_connection` | — | both | Home "Server:", Settings → Server profiles | implemented; profile logic unit-tested | `profile::tests`; signed-key path needs a rendezvous server |
| Profile isolation | (client-side) | — | `store.rs` keyed by profile id | — | both | — | **verified** (unit test) | `store::tests::profile_isolation_and_roundtrip` |
| Upstream server config string import | reversed base64 JSON | — | `ServerProfile::from_upstream_config_string` | — | both | Profile editor | implemented, unit-tested | `profile::tests::upstream_config_string_import` |
| Authentication: password (salted challenge) | `handle_hash`, `create_login_msg` | Host password | `session.rs` | — | both | Password dialog | **verified** | probe evidence; `password_hash_matches_upstream_scheme` |
| Authentication: wrong password / empty / approval / 2FA | `handle_login_error`, `Auth2FA` | host settings | `session.rs` LoginResponse handling | — | both | Password / 2FA dialogs | implemented but unverified (wrong-password path exercised manually in desktop testing) | — |
| Remember password + forget | `config.password` (salted hash) | — | `store.rs` secrets | storage | both | "Remember password?", peer menu, computer settings | **verified** (desktop: hash stored, reused) | `store::tests`; probe `RememberHash` event |
| Accounts / address book | `/api/login`, `/api/currentUser`, `/api/logout`, `/api/ab/*`, legacy `/api/ab` | API server providing them (RustDesk Pro or compatible) | `account.rs`, `http.rs` | outbound HTTPS | both | Home → Account & address book | implemented but unverified | `account::tests` (parsing only); no legitimate account service available for testing |
| Home / saved peers | — | — | `screens.rs` home, `store.rs` | — | both | Home | implemented (desktop-tested) | simulator screenshots |
| Display: monitor selection, Green cycling, picker | `SwitchDisplay`, `CaptureDisplays` | multi-monitor host | `session_view.rs` | — | both | Green, toolbar Monitor menu | implemented but unverified (test host has one monitor) | single-monitor path tested |
| Display: fit / 1:1 / zoom / pan, cursor mapping | (client-side) | — | `view.rs`, `session_view.rs`, `hwplane.rs`, `rdtv-media/src/smp.rs` (D29) | software decode, or Starfish + exported window with hardware decode | both | toolbar View | implemented; transform unit-tested | `view::tests` (letterbox, negative origins, portrait, zoom edges, keyboard shrink) |
| Remote resolution change (explicit only) | `Misc.change_display_resolution` | host permission | `SessionCommand::ChangeResolution` (command only) | — | both | — | planned (no UI yet; never automatic) | — |
| Picture quality: adaptive default, quality/FPS/codec controls | `OptionMessage.image_quality`, `custom_fps`, `SupportedDecoding.prefer` | — | session commands, computer settings | — | both | toolbar Quality, computer settings | implemented (desktop-tested quality messages reach host) | host log shows option update |
| Hardware decode H.264/H.265/VP9/AV1 | (client-side; NDL DirectMedia API 2) | host HW encoder for H.264/H.265; AV1 host encoder | `rdtv-media/src/ndl.rs`, `pipeline.rs` | `libNDL_directmedia.so.1`, launched via SAM | both (Dev Mode access unverified) | automatic | implemented but unverified | `ndl::tests` (struct layout, missing-library failure); needs TV |
| Software decode VP8/VP9 | (client-side; libvpx) | — | `vpx.rs` | CPU | both | automatic fallback | **verified** (x86 + ARM NEON under qemu, 1080p) | probe evidence |
| Codec failure → re-advertise | `update_supported_decodings` | — | `PipelineEvent::CodecFailed` → `SessionCommand::UpdateDecoding` | — | both | toast | implemented; pipeline unit-tested | `hevc_without_hardware_reports_codec_failure` |
| 1080p60 baseline / 4K60 | — | host encoder | — | TV decoder | — | — | implemented but unverified (no performance measurement possible without TV) | — |
| Input: Magic Remote pointer, five-way, colour keys, Back | SDL-webOS scancodes | — | `keys.rs`, `session_view.rs` | SDL-webOS backport | both | — | implemented but unverified (desktop simulator with F1–F4/F9 stand-ins) | `keys::tests` |
| Input: mouse buttons, wheel, drag, double-click, drag-lock | `MouseEvent` mask | — | `input.rs`, `session_view.rs` | — | both | toolbar Keys | **verified** (move + click on host); others implemented | `input::tests::mouse_mask_encoding`; probe evidence |
| Input: remote number buttons 0–9 | (client-side) | — | `keys::digit_of`, `App::digit_entry`, `keycode_char` | number keys delivered as SDL digit/keypad keys (SDL-webOS default XKB mapping; unverified on TV) | both | Home ID field, password/2FA/account dialogs, profile fields, session | implemented; simulator-tested (digits and keypad digits open ID entry pre-filled) | `remote_number_buttons_are_digits`, `keycode_chars` |
| Input: physical keyboard, modifiers, special keys | `KeyEvent` legacy mode | — | `keys.rs` `physical_to_remote` | USB/BT keyboard | both | — | implemented (desktop-tested), TV unverified | `hotkey_letters_are_sent_as_chars` |
| Held-input release on focus loss / mode change / disconnect | (client-side) | — | `HeldInput` | — | both | toolbar "Release stuck keys" | implemented, unit-tested | `held_input_released_once` |
| On-screen keyboard: full key set, US/HU, one-shot/locked modifiers, numpad, F-keys | Legacy `seq`/`chr`/`ControlKey` | — | `osk.rs` | — | both | Blue, toolbar Keyboard | implemented; logic unit-tested; Hungarian text **verified** on host (lower case all, upper case Ő Ű; see COMPATIBILITY) | `osk::tests`, probe evidence |
| OSK shortcut row (per computer) + Ctrl+Alt+Del | `ControlKey::CtrlAltDel` | host permission | `osk.rs`, `store.rs` | — | both | OSK top row | implemented but unverified on Windows | `ctrl_shift_esc_chord`, `privileged_actions_use_upstream_control_keys` |
| OSK shrink / overlay presentation | (client-side) | — | `view_rect` | software decode for shrink (D10) | both | toolbar View | **verified** in simulator (software decode) | screenshot; `keyboard_shrink_area_keeps_whole_monitor` |
| Audio playback + mute | `AudioFormat`, `AudioFrame` (Opus) | host audio capture | `audio.rs`, `audio_out.rs` | SDL audio / PulseAudio | both | toolbar Mute | implemented but unverified (test host had no audio device) | `audio::tests` |
| Administration: lock, lock-on-disconnect, Ctrl+Alt+Del, UAC | `LockScreen`, `lock_after_session_end`, `CtrlAltDel`, `Misc.uac` | host permissions; Windows | `input.rs`, `session.rs` | — | both | toolbar Keys, computer settings | implemented but unverified | needs Windows host |
| Session actions: refresh, reconnect | `refresh_video[_display]` | — | session commands | — | both | toolbar Keys → Refresh video | implemented (refresh used by pipeline recovery, verified keyframe requests reach host) | pipeline keyframe test |
| Lifecycle: default disconnect, Reconnect prompt, keep-in-background | (client-side) | — | `app.rs` focus handling | SDL focus/background events on webOS | both | Settings | implemented but unverified on TV | — |
| Packaging: reproducible IPK, CI, release | — | — | `scripts/package-ipk.py`, `.github/workflows/build.yml` | — | both routes documented | — | **verified** in CI (build, ELF audit, qemu self-test, IPK verify) | CI artifacts |
| Diagnostics, About/licenses | — | — | `screens.rs` diagnostics/about, `--self-test` | — | both | Settings | implemented | self-test in CI |
| File transfer/manager | — | — | disabled at login (`enable_file_transfer=No`) | — | — | — | **excluded by owner** | `login_message_disables_excluded_features` |
| Clipboard transfer/sync | — | — | disabled at login (`disable_clipboard=Yes`); inbound ignored | — | — | — | **excluded by owner** | same test |
| Multiple sessions / multi-monitor mosaic | — | — | single `active` session | — | — | — | **excluded by owner** | — |
| Chat, recording, screenshots | — | — | inbound messages ignored | — | — | — | **excluded by owner** | — |
| Terminal, tunnelling, remote printing | — | — | not requested (`ConnType::DEFAULT_CONN` only) | — | — | — | **excluded by owner** | — |
| Microphone, voice calls, gamepads | — | — | not implemented | — | — | — | **excluded by owner** | — |
| HDR, 120 Hz, gaming input capture | — | — | no HDR metadata, SDR only | — | — | — | **excluded by owner** | — |
| Incoming TV hosting / root-only integration | — | — | no listener, no root features | — | — | — | **excluded from scope** | — |
| Enterprise admin/SSO, virtual displays, LAN discovery/WoL UI | — | — | not implemented | — | — | — | not release requirements | — |

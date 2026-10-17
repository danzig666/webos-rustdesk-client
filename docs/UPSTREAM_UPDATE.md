# Updating to a newer RustDesk release

The client reuses RustDesk's `hbb_common` and `base` crates unmodified and ports a small set of client functions. Updating means moving the submodule, then re-checking each ported function against its upstream counterpart.

## 1. Pick the baseline

1. List the tags: `git ls-remote --tags https://github.com/rustdesk/rustdesk | awk -F/ '{print $3}' | grep -E '^[0-9]' | sort -V | tail`
2. Choose a **stable tag**, not `master` or `nightly`. Read its changelog and the diff of `src/client.rs` and `src/client/io_loop.rs` since the current pin.

## 2. Move the submodule

```sh
cd third_party/rustdesk
git fetch --depth 1 origin tag <TAG>
git checkout <TAG>
git submodule update --init --depth 1 libs/hbb_common
cd ../..
git add third_party/rustdesk
cargo update -p hbb_common -p base   # then review Cargo.lock changes
```

## 3. Re-check the ported functions

| This repository | Upstream origin (1.5.0) |
|---|---|
| `rdtv-core/src/transport.rs` `establish_by_id` | `src/client.rs` `Client::_start_inner` (TCP punch and relay branches only) |
| `transport.rs` timeout selection / direct connect | `src/client.rs` `Client::connect` |
| `transport.rs` `secure_connection` | `src/client.rs` `Client::secure_connection` (non-WebRTC branch) |
| `transport.rs` `request_relay`, `create_relay` | `src/client.rs` same names |
| `transport.rs` `get_rs_pk`, `decode_id_pk`, `create_symmetric_key_msg`, `get_next_nonkeyexchange_msg` | `src/common.rs` same names |
| `transport.rs` `Target::parse` / direct address port | `src/client.rs` `_start` (`is_ip_str`, `is_domain_port_str`, `RELAY_PORT + 1`) |
| `rdtv-core/src/session.rs` `option_message` | `src/client.rs` `LoginConfigHandler::get_option_message` |
| `session.rs` `login_msg` | `src/client.rs` `create_login_msg` |
| `session.rs` Hash / LoginResponse handling | `src/client.rs` `handle_hash`, `handle_login_error`; `src/client/io_loop.rs` `handle_msg_from_peer` |
| `session.rs` TestDelay echo, 30 s timeout | `src/client.rs` `handle_test_delay`; `io_loop.rs` main loop |
| `session.rs` `refresh_msg`, `switch_display_msgs` | `src/client.rs` `refresh`/`refresh_display`; `src/ui_session_interface.rs` `switch_display`, `capture_displays` |
| `rdtv-core/src/input.rs` | `src/common.rs` `MOUSE_*` constants; `src/server/input_service.rs` legacy keyboard handling |
| `rdtv-core/src/profile.rs` `effective_key`, `effective_api_server` | `src/common.rs` `get_key`, `get_api_server_` |
| `rdtv-core/src/account.rs` | `flutter/lib/models/user_model.dart`, `flutter/lib/common/hbbs/hbbs.dart`, `flutter/lib/models/ab_model.dart` |
| `rdtv-media/src/caps.rs`, `session.rs` `DecodeCaps` | `libs/scrap/src/common/codec.rs` `supported_decodings` and encoder negotiation |

For each row, diff the upstream function between the old and new tag and carry over any behaviour change. Pay particular attention to these areas:

- **Protocol:** `libs/base/protos/message.proto` and `libs/hbb_common/protos/rendezvous.proto`, for new required fields, renumbered fields, new `Misc` variants, or a changed `LoginRequest`/`LoginResponse`/`OptionMessage`.
- **Key exchange:** `kx_version` (`hbb_common::tcp::KX_VERSION_LATEST`, `kx_version_for`, `KxTranscript`, `KxParams` signing), and any change to `set_negotiated_key`.
- **Login:** password hashing, 2FA strings (`LOGIN_MSG_*`, `REQUIRE_2FA`), and approval-mode behaviour.
- **Codec negotiation:** in `libs/scrap/src/common/codec.rs`, the auto order and which abilities are assumed (VP9 is currently assumed always decodable). Also any new chroma or i444 rules.
- **Host input handling:** in `src/server/input_service.rs`, if Legacy keyboard mode semantics change.
- **Transports:** whether hosts still accept the classic TCP punch/relay transports. They are the only ones this client implements; WebRTC, UDP/KCP and IPv6 punch are not implemented.
- **Upstream config:** `hbb_common::config` changes that would make the client read or write files outside the app data directory.

## 4. Build and test

```sh
scripts/build-deps.sh host
cargo test --locked -p rdtv-core -p rdtv-media -p rdtv-app
WEBOS_SDK=... scripts/build-deps.sh webos
WEBOS_SDK=... cargo build --locked --release --target armv7-unknown-linux-gnueabi -p rdtv-app
WEBOS_SDK=... scripts/elf-audit.sh target/armv7-unknown-linux-gnueabi/release/remotedesk-tv deps-out/webos/lib/libSDL2-2.0.so.0
scripts/smoke-armel.sh target/armv7-unknown-linux-gnueabi/release/remotedesk-tv deps-out/webos/lib/libSDL2-2.0.so.0
```

## 5. Interoperability probe

Run the probe against an unmodified host of the **new** version, and of the owner's reported version (Windows 11 / RustDesk 1.5.0) if that differs:

```sh
cargo run -p rdtv-media --example probe -- <id-or-address> <password> 10
```

Record the result as a new file in `docs/evidence/`, following [`2026-10-17-linux-host-probe.md`](evidence/2026-10-17-linux-host-probe.md): host version and OS, route, login, negotiated codec, frames decoded and dropped, input, and anything that failed. A TV test of the release candidate is still required before a stable release.

## 6. Record

- Update the baseline (tag and commit) in `docs/DECISIONS.md`, `docs/SOURCES.md`, `crates/rdtv-app/src/screens.rs` (`UPSTREAM`) and `rdtv-core::transport::PROTOCOL_VERSION`. The version string must describe the behaviour actually ported.
- Note every behavioural change carried over, and every deliberate divergence, in `docs/DECISIONS.md`.
- Regenerate the notices: `python3 scripts/gen-notices.py`.

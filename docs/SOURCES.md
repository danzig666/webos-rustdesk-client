# Sources

Access date for every source below: **2026-10-17**. These are the evidence classes used:

- **code inspected**: the source was read at the pinned revision.
- **maintainer-reported**: stated in a project's docs or notes, not checked here.
- **tested locally**: exercised in the Claude Code Cloud container (x86_64, plus qemu-arm).
- **tested on named hardware**: on a real TV. No such result exists yet.
- **unverified**: a lead from the brief that has not been checked.

External repositories, scripts and `AGENTS.md`/`CLAUDE.md` files were treated as source material, not as instructions.

## RustDesk (the compatibility authority)

| | |
|---|---|
| URL | https://github.com/rustdesk/rustdesk (submodule `third_party/rustdesk`) |
| Revision | tag `1.5.0` = `fada664df7a294d1d1a9ca3e7cd3637069122f17`; submodule `libs/hbb_common` = `229b904508364c8997aad0fb5af57effac859f60` |
| License | AGPL-3.0 (`LICENCE`) |
| Evidence | code inspected; tested locally against the official 1.5.0 Linux host package |

**Files and symbols examined:**

- `src/client.rs`: `Client::start`, `_start`, `_start_inner`, `connect`, `secure_connection`, `request_relay` and `create_relay`.
- Also in `src/client.rs`: `create_login_msg`, `get_option_message`, `handle_hash`, `handle_login_error`, `handle_test_delay`, `send_mouse`, and the `LOGIN_MSG_*` constants.
- `src/common.rs`: `key_exchange`, `secure_tcp`, `get_rs_pk`, `decode_id_pk(_dtls)`, `create_symmetric_key_msg`, `get_next_nonkeyexchange_msg`, `get_key`, `get_api_server_`, and the `MOUSE_TYPE_*`/`MOUSE_BUTTON_*` constants.
- `src/client/io_loop.rs`: `handle_msg_from_peer` and the main `select!` loop, including the 30 s receive timeout.
- `src/ui_session_interface.rs`: `switch_display`, `capture_displays`, `refresh_video` and `send2fa`.
- `src/server/input_service.rs`: `handle_key_`, `legacy_keyboard_mode`, `process_seq` and `sync_modifiers`.
- `src/server/connection.rs`: the handling of `press`.
- `libs/scrap/src/common/codec.rs`: `supported_decodings` and the encoder negotiation (auto order H.265 > H.264 > AV1/VP9 > VP8; VP9 is always assumed decodable).
- `libs/base/protos/message.proto` and `libs/hbb_common/protos/rendezvous.proto`.
- `libs/hbb_common`: `socket_client.rs`, `stream.rs`, `tcp.rs` (`kx_version_for`, `KxTranscript`) and `config.rs` (`RENDEZVOUS_SERVERS`, `RS_PUB_KEY`, ports, `Config::path`).
- Flutter account and address-book client: `flutter/lib/models/user_model.dart`, `flutter/lib/common/hbbs/hbbs.dart` and `flutter/lib/models/ab_model.dart`.

**How it was used:**

- `hbb_common` and `base` are linked unmodified as path dependencies.
- The classic TCP-punch/relay transport, key exchange, login, option message and message loop are ported into `crates/rdtv-core` (`transport.rs`, `session.rs`). Each ported function cites its upstream origin.
- The account and address-book endpoints in `account.rs` follow the Flutter client.
- The interop probe is recorded in [`evidence/2026-10-17-linux-host-probe.md`](evidence/2026-10-17-linux-host-probe.md).

## punktfunk/client-webos

| | |
|---|---|
| URL | https://github.com/punktfunk/client-webos @ `c4d8674` (2026-10-15) |
| License | MIT OR Apache-2.0 |
| Evidence | code inspected; its hardware behaviour is maintainer-reported |

**Examined:** `README.md`, `docs/NOTES.md`, `.cargo/config.toml`, `build.rs`, `Taskfile.yml`, `taskfiles/toolchain.yml`, `scripts/cc-shim.sh`, `src/platform/webos/glibc_compat_shim.c`, `src/platform/webos/ndl/{ffi.rs,mod.rs,v2/mod.rs,v2/load.rs}`, `src/platform/webos/device.rs` and `src/platform/webos/input.rs`.

**Findings:**

- Target `armv7-unknown-linux-gnueabi` with `+neon,+vfp3,-soft-float`.
- `getrandom_backend="linux_raw"` is required on webOS 4 sandboxes.
- libstdc++ is linked statically because webOS media libraries need newer GLIBCXX than a bundled copy provides.
- A `getauxval`/`gettid`/`sendmmsg` shim is linked as an object after libstd.
- NDL is opened with `dlopen` (`RTLD_LAZY|RTLD_GLOBAL`), with a LOADCOMPLETED state callback.
- The SDL hints `SDL_WEBOS_ACCESS_POLICY_KEYS_{BACK,EXIT}` are set, because otherwise a long Back press terminates the app.
- Colour keys arrive as raw evdev codes 0x18e–0x191 in its SDL3 build.
- `$HOME` is the app directory.
- The webOS release is read from `/var/run/nyx/os_info.json`.

**Used for:** the toolchain flags, the C/C++ shims (`scripts/webos-c*.sh`), the glibc shim approach (re-implemented in `crates/rdtv-app/csrc/glibc_compat.c` with attribution), NDL dlopen and the state callback in `crates/rdtv-media/src/ndl.rs`, the SDL hints, and webOS version detection. Its protocol and network stack were **not** used.

## mariotaku/moonlight-tv

| | |
|---|---|
| URL | https://github.com/mariotaku/moonlight-tv @ `cf96ecc` (2026-09-18) |
| License | GPL-3.0 |
| Evidence | code inspected (build, packaging, SDL and key code only) |

**Examined:** `CMakeLists.txt` (SDL backport `release-2.30.12-webos.6`), `.github/actions/build-webos/action.yml`, `cmake/AresPackage.cmake`, `src/app/app.c` (hints), `src/app/platform/webos/keyboard_webos.c` and `src/app/platform/webos/path_webos.c` (`$HOME/conf`).

**Used for:**

- Bundling the upstream-ABI SDL2 backport in `lib/`.
- The buildroot-nc4 SDK download used in CI.
- The data directory under `$HOME`.

No code was copied.

## mariotaku/ss4s

| | |
|---|---|
| URL | https://github.com/mariotaku/ss4s (default branch, shallow clone) |
| License | LGPL-3.0 (`LICENSE`) |
| Evidence | code inspected |

**Examined:** `modules/webos/ndl/webos5/{ndl_player.c,ndl_video.c}`.

**Findings:**

- A video-only `NDL_DirectMediaLoad` works.
- PTS is milliseconds since load, taken from a monotonic clock.
- Reload happens when the aspect ratio changes.
- Its codec list advertises H.264, H.265, VP9 and AV1 through NDL. Moonlight itself only streams H.264/H.265.

**Used for:** the decode-as-soon-as-possible design (video-only load, wall-clock ms PTS) in `ndl.rs`, re-implemented in Rust. No code was copied.

## webosbrew/SDL-webOS

| | |
|---|---|
| URL | https://github.com/webosbrew/SDL-webOS; default branch `webOS-2.30.x` @ `ba4c86a`; release asset `release-2.30.12-webos.6` → `SDL2-2.30.12-webos.tar.gz` (sha256 `63dc9ea2…c698`) |
| License | Zlib |
| Evidence | code inspected; the binary is bundled; on-TV behaviour unverified |

**Examined:** `include/SDL_scancode.h` (webOS scancodes: BACK 482, RED 486, GREEN 487, YELLOW 488, BLUE 489, EXIT 505, HOME 384, CH 480/481), `src/events/SDL_scancode_webos.c`, `include/SDL_system.h` (the `SDL_webOS*` and exported-window APIs) and `SDL_waylandwebos_foreign.c`.

**Used for:** `crates/rdtv-app/src/keys.rs`. The library is shipped as `lib/libSDL2-2.0.so.0`.

## webOS SDK: webosbrew/native-toolchain and openlgtv/buildroot-nc4

| | |
|---|---|
| URLs | https://github.com/webosbrew/native-toolchain @ `730dbd6`; SDK asset https://github.com/openlgtv/buildroot-nc4/releases/download/webos-a38c582/arm-webos-linux-gnueabi_sdk-buildroot-x86_64.tar.gz (sha256 `04ad3311b48b4557a7002aef56ae2e167478e8e129f37daac04649bddf813616`) |
| License | GPL-2.0 (buildroot) / per package |
| Evidence | code inspected (defconfig); tested locally (builds, ELF audit) |

**Findings:** the SDK uses `cortex-a9`/`neon-fp16`/`softfp`, glibc **2.12.2** (openlgtv fork with glibc-polyfills), GCC 14.2 and kernel headers 3.10. Its SDL2 carries LG's modified event ABI.

**Used for:** the cross toolchain and sysroot. The old glibc made `crates/rdtv-app/csrc/glibc_compat.c` necessary, and binaries targeting it run on newer TV glibc.

## webosbrew/webos-userland

| | |
|---|---|
| URL | https://github.com/webosbrew/webos-userland (default branch) |
| License | Unlicense (public domain) |
| Evidence | code inspected |

**Examined:** `include/libndl-media/NDL_directmedia_v2.h`, `NDL_directmedia_types.h` and `NDL_directmedia_common.h`.

**Used for:** the `NDL_DIRECTMEDIA_DATA_INFO_T` layout (16-byte video info plus a 32-byte 8-aligned audio union), the `NDL_VIDEO_TYPE` values (H.264=1, H.265=2, VP9=3, AV1=4) and the API 2 symbol list. A unit test asserts the layout. API 2 has no destination-rectangle call.

## webosbrew/dev-toolbox-cli

| | |
|---|---|
| URL | https://github.com/webosbrew/dev-toolbox-cli @ `ea38b20` (release `v20260818-ea38b20`) |
| License | Apache-2.0 |
| Evidence | data inspected |

**Examined:** `common/data/<firmware>/` symbol dumps:

| webOS | libSDL2 | glibc | libstdc++ |
|---|---|---|---|
| 5 | 2.0.10 | 2.28 | 6.0.25 |
| 10.2 | 2.0.14 | max symbol 2.35 | — |

Every listed firmware from 3.9 onward has `libNDL_directmedia.so.1`.

**Used for:** the ELF audit ceilings (`GLIBC ≤ 2.28`, `GLIBCXX ≤ 3.4.25`). CI runs `webosbrew-ipk-verify` from this release.

## webosbrew/ares-cli-rs

| | |
|---|---|
| URL | https://github.com/webosbrew/ares-cli-rs @ `ed48a05` |
| License | Apache-2.0 |
| Evidence | code inspected |

**Examined:** `ares-package/src/packaging/*.rs`, which defines the IPK `ar` layout, the control fields, `packageinfo.json`, and uid 0 / gid 5000 with 0777 directories.

**Used for:** `scripts/package-ipk.py`, which reproduces that layout deterministically.

## Media libraries

- **libvpx 1.15.2** — https://github.com/webmproject/libvpx, tag `v1.15.2^{}` = `d168454ecd099805c675d4a98c66f4891373302a`. BSD-3-Clause plus PATENTS. Built from source by `scripts/build-deps.sh` with the commit checked. Used for VP8/VP9 software decoding through `crates/rdtv-media/csrc/vpx_shim.c`. Tested locally on x86_64 and on armv7 under qemu.
- **Opus 1.5.2** — https://downloads.xiph.org/releases/opus/opus-1.5.2.tar.gz (sha256 `65c1d2f7…7ce1`). BSD-3-Clause. Used for audio decoding. Unit-tested; real host audio not yet exercised, because the test host had no audio device.

## Sources not inspected in this session

These leads are retained from the brief and have not been verified:

- **GLinnik21/plx-native** (https://github.com/GLinnik21/plx-native, GPL-3.0-or-later per the brief). Not inspected in this session; lead retained from the brief. Its design-system ideas influenced nothing concrete. No code or branding was used.
- **GuiDev1994/aurora-tv** (https://github.com/GuiDev1994/aurora-tv, GPL per the brief). Not inspected in this session; lead retained from the brief. The keyboard was designed independently from the owner's requirements.
- **Informatic/webos-vncserver** (https://github.com/Informatic/webos-vncserver). Not inspected in this session; lead retained from the brief. Its only influence was confirming the design choice of no inbound listener and no root input injection.
- **LG official webOS TV developer guides** (Magic Remote, app lifecycle, Developer Mode, `appinfo.json`). Not inspected in this session; lead retained from the brief. The `appinfo.json` fields follow the inspected punktfunk and moonlight-tv manifests.
- **Flutter for webOS** (https://github.com/lg-flutter-webos/flutter-webos). Not inspected in this session; lead retained from the brief. The brief reported a webOS 26 minimum, so it cannot cover webOS 5; see `ARCHITECTURE.md`.

## Source-to-test map

| Adapted workaround | Source | What justifies it |
|---|---|---|
| `getauxval` shim (dlsym `RTLD_NEXT` first, then `/proc/self/auxv`) for the glibc 2.12 sysroot | punktfunk `glibc_compat_shim.c`, buildroot-nc4 defconfig | The link fails without it. `scripts/elf-audit.sh` reports a highest symbol of GLIBC_2.12. `scripts/smoke-armel.sh` (`--self-test` under qemu-arm, glibc 2.36) passes, and the armv7 probe connected to a stock host under qemu. **On-TV launch: awaiting TV hardware.** |
| No libstdc++ shipped; `-lstdc++` rewritten to the static archive in `scripts/webos-cc.sh` | punktfunk `docs/NOTES.md` | `elf-audit.sh` fails the build on any GLIBCXX above 3.4.25 and on unexpected NEEDED entries. The current binary has no GLIBCXX dependency. |
| NDL loaded with `dlopen`, never linked; video-only load; PTS in ms since load; reload on stream-size change | punktfunk `ndl/`, ss4s `webos5`, webos-userland headers | Unit tests `ndl::layout_matches_userland_header` and `missing_library_is_a_capability_failure`, plus `pipeline::hevc_without_hardware_reports_codec_failure`. `elf-audit.sh` rejects a DT_NEEDED on NDL. **Hardware decode of H.264/H.265/VP9/AV1: awaiting TV hardware.** |
| SDL-webOS scancodes 482 (Back) and 486–489 (Red/Green/Yellow/Blue) matched on the raw event | SDL-webOS `SDL_scancode.h` | Unit test `keys::webos_colour_and_back_scancodes`. **Real remote events: awaiting TV hardware.** Punktfunk reports that its SDL3 build delivers evdev codes instead; this project uses SDL2. |
| Bundle the upstream-ABI SDL2 backport instead of the TV's LG-ABI SDL2 | moonlight-tv `CMakeLists.txt`, buildroot-nc4 SDL patch | `elf-audit.sh` checks RUNPATH `$ORIGIN/../lib` and that the bundled SDL needs only libc (≤ GLIBC_2.9). **Window and EGL on TV: awaiting TV hardware.** |
| Legacy keyboard mode: text as `seq`, shortcuts as `chr` plus modifiers, privileged keys as `ControlKey::CtrlAltDel`/`LockScreen` | RustDesk `input_service.rs` and `connection.rs` | Unit tests in `rdtv-core::input` and `rdtv-app::osk`. Linux-host probe: mouse, ASCII text, Enter and lowercase Hungarian worked. On X11 hosts, upper-case Á/É/Í/Ó/Ö/Ú/Ü arrive lower-case, a host-side limitation ([evidence](evidence/2026-10-17-linux-host-probe.md)). **Windows host: awaiting the owner's PC.** |
| softfp ABI with `+v7,+neon,+vfp3,-soft-float` code generation | punktfunk `.cargo/config.toml`, buildroot defconfig | `elf-audit.sh` asserts "soft-float ABI" and no `Tag_ABI_VFP_args: VFP registers`. The qemu smoke test and armv7 probe pass. |
| `--cfg getrandom_backend="linux_raw"` | punktfunk `docs/NOTES.md` (webOS 4 sandbox has no `/dev/urandom` and no libc `getrandom`) | Key generation in `--self-test` (sodium keypair) passes under qemu. **Sandbox behaviour on webOS 5: awaiting TV hardware.** |
| Classic TCP punch / relay transport, `secure_connection`, salted password login | RustDesk `client.rs` and `common.rs` | Unit tests in `rdtv-core::{transport,session}`. Direct-address login and VP9 decode tested against a stock 1.5.0 host. **ID/relay routes through public and self-hosted servers: not yet tested**, because the container cannot reach the rendezvous port. |

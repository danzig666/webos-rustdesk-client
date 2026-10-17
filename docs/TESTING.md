# Test report

Each result says what kind of evidence it is: **unit test (CI)**, **emulation (CI/local)**,
**real host, desktop/emulated client**, or **real TV**. No real-TV results exist yet.

## Automated (every push, GitHub Actions `build` workflow)

| Check | Kind | What it proves |
|---|---|---|
| `cargo fmt --check`, clippy (project crates) | static | code hygiene |
| `cargo test` (rdtv-core 24, rdtv-media 9, rdtv-app 17 tests) | unit test | target parsing, key decoding, profile key/API derivation, upstream config import, profile isolation and roundtrip, secret redaction in exports, 0600 files, password hash scheme, login option message (clipboard/file transfer disabled, codecs), codec preference fallback, malformed cursor rejection, mouse mask encoding, chords, Hungarian text as `seq`, held-input release, Ctrl+Alt+Del/LockScreen control keys, reconnect classification and the 30 s bounded plan, HTTP parsing (chunked), account response parsing, legacy address book, NDL struct layout and missing-library behaviour, codec caps policy, keyframe gating, HEVC-without-hardware failure path, audio conversion/underrun/latency cap, view transform (letterbox, negative origins, retina, zoom edges, 1:1, keyboard shrink, portrait), geometric focus navigation, OSK Hungarian/AltGr/one-shot/locked/Ctrl+Shift+Esc/Ctrl+C/local mode, webOS colour/Back scancodes, keycode→char |
| Cross-compile for `armv7-unknown-linux-gnueabi` with the webOS SDK | build | the TV binary links against the SDK sysroot |
| `scripts/elf-audit.sh` | static | ELF32 ARM, softfp ABI, `/lib/ld-linux.so.3`, NEEDED allowlist, glibc ≤ 2.28 (actual max 2.12), no GLIBCXX, RUNPATH `$ORIGIN/../lib`, NDL not linked |
| `scripts/smoke-armel.sh` (`--self-test` under qemu-arm, Debian armel glibc 2.36) | emulation | the ARM binary starts on a modern glibc and its static libvpx/libopus/libsodium, key decoding and storage work |
| `webosbrew-ipk-verify --fw-releases '>=5'` | static | IPK checked against webOS firmware symbol dumps |

## Manual, in the development container (2026-10-17)

| Test | Kind | Result |
|---|---|---|
| Probe (x86_64) ↔ stock RustDesk 1.5.0 Linux host, direct IP | real host, desktop client | connected, login, VP9 1080p decode, cursor, click, text, close (see [evidence](evidence/2026-10-17-linux-host-probe.md)) |
| Probe (ARM build, qemu-arm armel) ↔ same host | real host, emulated ARM client | connected, login, 35/35 VP9 1080p frames decoded with NEON libvpx, input delivered |
| Full app (desktop simulator) driven by synthetic keypresses: Home → ID editor → Connect → password dialog → editor → live session → OSK → toolbar | real host, desktop client | all steps worked. Screenshots showed the live remote desktop, OSK shrink mode with aspect preserved, and the toolbar |
| Hungarian text through the host | real host | lower case correct; upper case `Ő Ű` correct, `Á É Í Ó Ö Ú Ü` lower-cased by the Linux/X11 host (host-side, see COMPATIBILITY) |
| SDK glibc 2.12 runtime under qemu | emulation | does not run (even a C hello world segfaults), so the armel userland is used instead; this says nothing about TVs |

## Waiting for hardware

Everything in [NONROOT_CHECKLIST.md](NONROOT_CHECKLIST.md) and the release gates in
[COMPATIBILITY.md](COMPATIBILITY.md): launcher start, rendering, remote keys, NDL decode and
composition, audio, lifecycle, Developer Mode, webOS 5, Windows host administration cases,
performance (1080p60, 4K60), the 8-hour soak and 50 connect/disconnect cycles.

## Reproducing the host interop test locally

```sh
# Host: stock RustDesk .deb on a virtual display, direct IP access, permanent password.
Xvfb :99 -screen 0 1920x1080x24 &
DISPLAY=:99 LANG=C.UTF-8 rustdesk --server &
rustdesk --password <pw>          # with the server running
# Options in ~/.config/rustdesk/RustDesk2.toml: direct-server='Y', approve-mode='password',
# verification-method='use-permanent-password'

# Client:
scripts/build-deps.sh host
cargo run -p rdtv-media --example probe -- 127.0.0.1 <pw> 10
```

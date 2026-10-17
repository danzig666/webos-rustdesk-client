# Architecture decision record

Status: **accepted** (2026-10-17). It was decided from source and build evidence. On-TV validation
of the capability probe is still pending (no TV access in the development environment).

## Context

The TV must drive stock RustDesk hosts on webOS 5.x and newer, in both Developer Mode and
rooted installs, with hardware video, polished five-way and Magic Remote UX and a full keyboard.

## Options compared

| | A: RustDesk core + adapted Flutter frontend | **B: RustDesk core + native SDL2 TV frontend** |
|---|---|---|
| Device coverage | LG's official Flutter for webOS targets new (webOS 26-era) TVs per the brief (not re-verified). A custom embedder for webOS 5 would be a new, unproven, owned platform port | SDL2 webOS port (webosbrew backport) is used by Moonlight TV on webOS 3+; the buildroot-nc4 SDK targets every webOS generation |
| Reuse of RustDesk UI | Dart screens exist but are desktop/mobile, plugin-heavy (window manager, clipboard, file dialogs) | None; TV UI written for remotes |
| Video | Needs a platform view / texture bridge to NDL (unproven) | NDL hardware plane under a transparent SDL window (proven pattern: punktfunk, Moonlight TV) |
| Input | Flutter key events → needs webOS key mapping in the embedder | SDL-webOS scancodes for colour keys/Back, documented |
| Memory / startup | Flutter engine + Dart VM | One ~12 MB stripped binary + 1.3 MB SDL2 |
| Maintenance | Bridge + embedder + plugins | One Rust codebase; protocol crates unchanged |

## Decision

**Option B.** The client is three crates:

```
rdtv-app   SDL2 frontend: ui toolkit, screens, OSK, session view, input router, audio out
   │
   ├── rdtv-media  NDL hardware decode (dlopen), libvpx VP8/VP9, libopus, decode pipeline thread
   │
   └── rdtv-core   transport (rendezvous/punch/relay/key exchange), login, session loop,
                   input messages, profiles/store, reconnect policy, account API client
                       │
                       └── upstream hbb_common + base (RustDesk 1.5.0, unmodified submodule)
```

## Threads and data flow

```
SDL main thread ──(SessionCommand: input, monitor, quality, disconnect)──► tokio runtime (2 workers)
     ▲                                                                     │ session task
     │ AppMsg::Session / AppMsg::Pipeline (mpsc, tagged with generation)   │ (hbb_common Stream)
     │                                                                     ▼
     │                                         MediaSink::video ──► VideoPipeline thread
     │                                         (bounded 120, never blocks)  ├─ NDL (hardware plane)
     │◄── newest software picture (single-slot mailbox) ────────────────────┘─ libvpx → I420
     │
SDL audio callback ◄── AudioRing (bounded 250 ms) ◄── MediaSink::audio (Opus decode)
```

* Every session has a **generation id**. Events from a stale session are dropped, so an old session
  can never change a new one or another profile's data.
* Networking and decoding never run on the UI thread. Blocking NDL calls stay on the video thread.
  Teardown waits a bounded 3 s and then detaches (logged).
* The video queue drops on overflow, flags a resync and requests a keyframe through upstream's
  `refresh_video[_display]`. Decoding never skips interdependent frames: after a drop it waits for a
  keyframe.

## Platform boundary

* No upstream code is patched. Desktop-only facilities (capture, input injection, clipboard, file
  transfer, tray, IPC, services) are simply absent: `rdtv-core` uses only `hbb_common`
  (transport/crypto/config) and `base` (protobuf messages).
* No fake `target_os`. The TV build is plain `armv7-unknown-linux-gnueabi`.
* Optional vendor APIs (NDL) are `dlopen`ed with explicit symbol checks. Missing symbols become a
  capability failure (`caps::Platform.ndl_error`), never a startup crash.
* Excluded features are disabled at the protocol boundary: the login sends `disable_clipboard=Yes`
  and `enable_file_transfer=No`, and inbound clipboard/file/chat/terminal messages are ignored.

## Capability probe status

The brief asks for an installable capability application before the full UI. Here the full
client doubles as the probe:

* `remotedesk-tv --self-test` runs headless checks (decoders, crypto, storage, NDL probe, advertised
  codecs). It runs in CI under qemu on an armel userland, and can be run over SSH on a TV.
* Settings → Diagnostics shows the same information on screen and saves a report.

What still needs a TV: window creation through the launcher, rendering, remote key codes, audio
output, NDL load/decode/composition, Developer Mode permissions. See
[NONROOT_CHECKLIST.md](NONROOT_CHECKLIST.md) and [COMPATIBILITY.md](COMPATIBILITY.md).

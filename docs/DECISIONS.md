# Decisions

Two kinds of entries:

* **Confirmed owner decisions**: fixed product scope from the owner's brief (October 17, 2026).
  They are not provisional and are not reopened by a generic parity checklist.
* **Accepted implementation defaults**: choices made during implementation within that scope,
  each with its reason. They can be revised when evidence (especially TV hardware results)
  says otherwise.

## Confirmed owner decisions

### Scope

* Outbound RustDesk-compatible remote desktop **client** for consumer LG webOS TVs, **webOS 5.x and
  newer**. Both installation routes are first-class: non-rooted Developer Mode, and rooted /
  Homebrew Channel. Root must not be required.
* Stock, unmodified RustDesk hosts (Windows, Linux, macOS). First interoperability target:
  **Windows 11 with RustDesk 1.5.0** (owner-reported).
* Public RustDesk service and self-hosted OSS servers. Accounts and address books when the configured
  service actually provides them; never faked.
* **Excluded**: file transfer/manager, clipboard sync in either direction, TV-local clipboard,
  simultaneous sessions, multi-monitor mosaics, chat, recording, screenshots, terminal, tunnelling,
  remote printing, microphone/voice calls, gamepad forwarding, gaming input capture, HDR, 120 Hz,
  incoming TV hosting, guest profiles, local access locks. Remote-only copy/paste chords remain.

### Owner-reported hardware and environment

| Item | Value (owner-reported; not yet measured) |
|---|---|
| Initial TV | LG C3, rooted |
| webOS release | 10.2.2 |
| Firmware | 33.23.05 |
| Platform id | O22N |
| First host | Windows 11, RustDesk 1.5.0 |

### Product behaviour

| Topic | Decision | Where implemented |
|---|---|---|
| Sessions | Exactly one; connecting elsewhere ends the old one with confirmation when it is live | `app.rs` `request_connect`, `Modal::Confirm(SwitchSession)` |
| Server profiles | Separate, switchable from Home; everything scoped per profile; same peer ID on two profiles shares nothing | `store.rs` (keyed by profile id), Home "Server:" button |
| Connection entry | ID/password primary; direct address when the host allows | `transport::Target::parse` |
| Home | Opens on launch; ID entry, favourites, recents (+ address book) | `screens.rs` `home` |
| Remember password | Simple explicit "Remember password?" toggle; forget/delete controls; no PIN/master password | password dialog, peer menu, computer settings |
| Lock policy | Default leaves lock state unchanged; per-computer "Lock on disconnect"; separate "Lock remote computer" action | `PeerSettings.lock_on_disconnect`, toolbar Keys menu |
| Windows admin | Send Ctrl+Alt+Delete through upstream `ControlKey::CtrlAltDel` | `input::ctrl_alt_del` |
| Resolution | Never changed automatically; only an explicit user action may | no automatic `change_display_resolution` anywhere |
| Image | Sharp SDR text, stable latency; no HDR/120 Hz | no HDR metadata path |
| Audio | On by default; toolbar mute remembered per computer; TV mute never changes PC volume | `AudioRing::set_muted`, `PeerSettings.muted` |
| Localization | English UI; US English and Hungarian keyboard layouts with quick switching | `osk.rs` |

### Remote-button contract (while the desktop has input focus)

| Input | Action |
|---|---|
| Magic Remote pointing | Absolute pointing through the shared transform |
| OK / click | Left click; double-click and drag-lock in the toolbar |
| D-pad | Virtual mouse by default; local menus use focus navigation |
| Yellow | Toggle D-pad Mouse / Arrow-key mode, with indicator |
| Green | Next monitor, wrapping; monitor name/number shown briefly |
| Red | Right click |
| Blue | Open/close on-screen keyboard |
| Short Back | Close top local panel; else show/hide toolbar; never Escape, never disconnect |
| Wheel | Scroll |
| Escape | Keyboard key / Keys menu |
| Home / long Back | Left to the TV |

### Keyboard, view, lifecycle

* Full on-screen keyboard with the specified key set, one-shot and locked modifiers, US and
  Hungarian layouts, compact/expanded, numeric keypad, per-computer shortcut row (defaults Alt+Tab,
  Ctrl+Shift+Esc, Win+D, Ctrl+C, Ctrl+V; macOS variants), Ctrl+Alt+Del as a separate host action.
* Default keyboard presentation **shrinks** the picture above the keyboard (aspect kept, whole
  monitor visible); alternative **overlay**; remembered.
* Fit to screen initially; 1:1, zoom and pan as local view changes.
* Toolbar auto-hides after **5 s** of inactivity.
* Leaving the app **disconnects by default**, then a prominent **Reconnect** on return. Optional
  **Keep connected in background** pauses input, mutes audio and stops video.
* Unexpected loss: automatic reconnect for up to **30 s** with countdown and Cancel, then Retry.
  Never after intentional disconnect, cancel or leave-app disconnect.
* Physical keyboard escape chord **Ctrl+Alt+Shift+Space** opens local controls (configurable).

## Accepted implementation defaults

| # | Decision | Reason / evidence |
|---|---|---|
| D1 | **Architecture: upstream RustDesk Rust core + native SDL2 TV frontend** (Option B). See [ARCHITECTURE.md](ARCHITECTURE.md). | The brief's accepted starting architecture. Flutter for webOS needs webOS 26-era TVs (brief lead, not re-verified), which cannot cover webOS 5. |
| D2 | **Baseline RustDesk 1.5.0** (`fada664`), `hbb_common` `229b9045`, as a git submodule. | Latest stable tag on 2026-10-17 (`git ls-remote`), and the owner's host version. |
| D3 | Depend on upstream `hbb_common` + `base` unchanged; **port** the outbound client functions (`src/client.rs`, `src/client/io_loop.rs`) into `rdtv-core` with attribution, rather than linking the desktop `rustdesk` crate. | The `rustdesk` crate drags capture, input injection, clipboard, GUI, IPC and service code. Porting keeps protocol logic identical and the patch set to upstream at zero lines. |
| D4 | Transport subset: **classic TCP hole punch + relay + direct address**. No WebRTC, UDP/KCP or IPv6 punch transports, no WebSocket rendezvous. | These are still upstream's fallback paths and every stock host accepts them. Verified against stock 1.5.0 (direct address). Relay and P2P via the public server are untested here (the container has no outbound 21116/21117). |
| D5 | Keyboard delivery in upstream **Legacy** mode: text as `seq`, shortcuts as `chr`+modifiers, special keys as `ControlKey`. Modifiers travel with each event. | Host-layout independent text (Hungarian works without changing the host layout); no host keys held while navigating the OSK. Verified on a Linux host, with one host-side limitation (see COMPATIBILITY.md). |
| D6 | **Layout setting = local OSK layout only.** It decides which characters local keys produce; the host's layout is never changed. | Owner requirement: "do not silently change the host's layout". |
| D7 | Video: **NDL DirectMedia (API 2)** hardware decoding for H.264, H.265, VP9 and AV1, `dlopen`ed at runtime; **libvpx** VP8/VP9 software fallback (static). | The owner asked for hardware codecs (C3 has VP9/AV1 hardware). Hosts treat VP9 as always decodable (`libs/scrap/src/common/codec.rs`), so a VP9 path is mandatory. |
| D8 | NDL load is **video-only**, PTS = milliseconds since load, presented as soon as possible. Feeding waits for `LOADCOMPLETED` or 300 ms. | ss4s/Moonlight TV approach (low latency); punktfunk's 300 ms "feed anyway". No frame pacing wanted for a desktop. |
| D9 | Codecs advertised: H.264/H.265 when NDL is present; **AV1 only in hardware and only on webOS ≥ 6**; VP9 to NDL when hardware is on, else libvpx. A codec that fails to load or feed is withdrawn and re-advertised (`Misc.option.supported_decoding`), as upstream does after decoder failures. | NDL has no capability query. AV1 has no software fallback here, so advertising it where unsupported would break sessions. |
| D10 | With hardware decoding the TV presents the picture **full-screen (Fit)**; 1:1, zoom and keyboard-shrink need software decoding. The keyboard overlays the picture then, and the UI says so. | NDL API 2 has no destination rectangle (`webos-userland` header). Exported-window viewport control (`SDL_webOSSetExportedWindow` + `NDL_DirectMediaSetWindowId`) is unproven and not implemented. A setting flag is reserved for it. |
| D11 | **Audio: libopus → SDL audio** (PulseAudio on webOS), 48 kHz stereo, a bounded 250 ms ring with drop-oldest. | Works for every codec path; NDL's audio plane would tie audio to the video pipeline. |
| D12 | **SDL2 2.30.12 webOS backport (upstream ABI) bundled** in `lib/`, linked through `RUNPATH $ORIGIN/../lib`. | The TV's own SDL2 uses LG's modified event struct ABI (`inputSource` field), which would break `sdl2-sys`. Moonlight TV ships the same backport. |
| D13 | Toolchain: `armv7-unknown-linux-gnueabi`, `+v7,+neon,+vfp3,-soft-float` (softfp ABI, hardware FP codegen), `getrandom_backend="linux_raw"`; buildroot-nc4 `webos-a38c582` SDK (gcc 14.2, glibc 2.12 sysroot). | Matches the SDK float ABI (verified by the ELF audit: "soft-float ABI", no `Tag_ABI_VFP_args`). Flags as used by punktfunk. NEON is assumed on all webOS 5+ SoCs (unverified on webOS 5 hardware). |
| D14 | `getauxval` compat object: calls the TV's real `getauxval` via `dlsym(RTLD_NEXT)`, else parses `/proc/self/auxv`. | Rust std needs it, the 2.12 sysroot lacks it, and every webOS 5+ TV has it (glibc ≥ 2.28). Real semantics, no fabricated success. |
| D15 | Text rendering: fontdue + embedded Inter (Regular/SemiBold, since 0.4.1; owner asked for a more modern look) with DejaVu Sans/Bold as per-glyph fallback for symbols; own immediate-mode focus toolkit on the SDL renderer. | No dependency on TV font/TTF libraries. DejaVu covers Hungarian ő/ű. The toolkit is small, testable and handles five-way and pointer focus uniformly. |
| D16 | UI logical space **1920×1080** (SDL logical size); body text 28 px, small 24 px, titles 44 px; interface size setting 85–130 %. | Brief's starting point; to validate on the TV. |
| D17 | Remote cursor: the Magic Remote pointer (drawn by the TV) is the cursor in pointer mode; the host's cursor image is drawn at the virtual mouse position in D-pad mouse mode. | Avoids double cursors and does not fight the TV's system pointer. |
| D18 | Physical keyboard text is derived from SDL keycodes + Shift (US for ASCII; non-ASCII keycodes used directly). `SDL_StartTextInput` is not used. | `SDL_StartTextInput` would raise LG's own virtual keyboard. |
| D19 | Remote arrows/OK and keyboard arrows/Enter are indistinguishable in SDL2. Typing on a physical keyboard switches arrows/Enter to key mode for 10 s. | No `inputSource` in the upstream SDL ABI (D12). |
| D20 | Leaving the app: input stops immediately on focus loss; the disconnect (or background mode) applies after **2.5 s** without focus coming back. | "A transient system overlay need not count as leaving the app." |
| D21 | Reconnect backoff 1, 2, 4, 6, 8 s… within 30 s. The last successful salted hash is kept **in memory only** for automatic reconnects. | Bounded backoff. Reconnect without re-typing the password, but nothing persisted without opt-in. |
| D22 | Persistence: `$HOME/data/state.json` (no secrets) and `secrets.json` (salted hashes, tokens), both 0600, atomic writes. See [SECURITY.md](SECURITY.md). | Upstream-compatible remembered-password representation; no reversible "encryption" marketed. |
| D23 | Working name **RemoteDesk TV**, app id **`io.github.danzig666.remotedesktv`** (owner-controlled namespace), owner-supplied icon. | Brief: neutral placeholder name; not legally cleared. See [LICENSING.md](LICENSING.md). |
| D24 | IPK built by the repository's own deterministic packager (`scripts/package-ipk.py`, ares-package layout) and verified with `webosbrew-ipk-verify` in CI. | No LG SDK or npm tooling needed; reproducible. |
| D25 | Releases are ordinary GitHub releases (no "test build" label) carrying only the IPK and the source archive. | Superseded the earlier pre-release rule after the owner tested the app on an LG C3 and asked for it (2026-10-17). Build evidence stays in the workflow artifact. |
| D26 | Debug logging is opt-in (`RDTV_DEBUG=1`); third-party info logs are suppressed, warnings kept. | Upstream crates log per packet at info level, which wastes TV storage. |
| D28 | Remote **number buttons** are handled as digit/keypad keys. Outside an editor a digit opens the relevant field (Home ID, password, 2FA/verification code, focused profile field) pre-filled with it; in a session digits are typed on the computer and do not switch the D-pad into keyboard mode. | The SDL-webOS fork maps only colour/navigation/media buttons specially (`src/events/SDL_scancode_webos.c`), so digits use the default keymap. Owner request (2026-10-17). |
| D29 | Hardware video **placement** (keyboard shrink, 1:1, zoom with hardware decoding) uses LG's **Starfish** pipeline (`libplayerAPIs`, dlopen) with the exported window id in the Load payload, then `SDL_webOSSetExportedWindow`. NDL stays the full-screen fallback when Starfish or the exported window is unavailable, or placement is switched off. Long-press right click shows no banner; the persistent "Mouse · Back: menu" chip was removed. | NDL API 2 has no destination rectangle and `NDL_DirectMediaSetWindowId` did not move the picture on the owner's TV (0.1.3). Starfish + exported window is what Cobalt, ss4s and plx-native use. Owner feedback (2026-10-17). Unverified on TV. |
| D27 | Account/address book: upstream `/api/login` (account, `email_code`/`tfa_code` challenges), `/api/currentUser`, `/api/logout`, Pro `/api/ab/personal` + `/api/ab/peers`, legacy `/api/ab`. Address-book password hashes are **not** used. Read-only address book (no editing UI). | Source-grounded endpoints. Using shared hashes silently is a trust decision not made by the owner. Editing is beyond the required scope. |

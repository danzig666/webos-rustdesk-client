## RemoteDesk TV: test build

Community RustDesk-compatible remote desktop client for LG webOS TVs (webOS 5.x and newer). It
connects **out** to a computer running the normal RustDesk app (protocol baseline RustDesk 1.5.0).

> ⚠️ **Experimental test build. Not yet verified on any TV.** It was built and checked in CI
> (cross-compile, ELF/ABI audit, emulated self-test, IPK verification) and tested against a stock
> RustDesk 1.5.0 Linux host from a desktop and an emulated-ARM build. TV launch, remote keys, audio,
> hardware decoding and Developer Mode still need real hardware: see `docs/NONROOT_CHECKLIST.md`
> and please report results.

### Install

* **Rooted TV / Homebrew Channel:** webOS Dev Manager, or `scp` + `luna-send` (see `docs/INSTALL_ROOTED.md`).
* **Developer Mode:** `ares-install --device tv io.github.danzig666.remotedesktv_<ver>_arm.ipk` (see `docs/INSTALL_DEVMODE.md`).

### What is in it

* ID connections through the public RustDesk server or self-hosted servers (separate profiles), and direct IP.
* Password login with optional "Remember password?", 2FA, waiting for remote approval.
* Hardware decoding via the TV's NDL DirectMedia (H.264, H.265, VP9, AV1), software VP9/VP8 fallback.
* Remote audio with mute.
* Magic Remote pointing; Red right click, Green next monitor, Yellow mouse/arrow mode, Blue keyboard, Back toolbar.
* Full on-screen keyboard: US and Hungarian layouts, Ctrl/Alt/AltGr/Shift/Win (once or locked), F-keys, numpad, per-computer shortcuts, Ctrl+Alt+Del.
* Fit, 1:1 and zoom (1:1 and zoom with software decoding); keyboard shrink or overlay.
* Default disconnect when leaving the app, with Reconnect; optional keep-connected; 30 s automatic reconnect.
* Accounts and address book where the server provides them.

### Files

* `io.github.danzig666.remotedesktv_<ver>_arm.ipk`: the app.
* `SHA256SUMS.txt`: checksum.
* `*.manifest.json`: Homebrew repository manifest.
* `sbom.cdx.json`, `THIRD-PARTY-NOTICES.txt`: software bill of materials and notices.
* `elf-audit.txt`, `smoke-test.txt`, `ipk-verify.md`: build checks.
* `remotedesk-tv-<ver>-source.tar.gz`: complete corresponding source (AGPL-3.0), including the pinned RustDesk submodule.

Not affiliated with or endorsed by RustDesk or LG.

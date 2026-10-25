## RustCouch

Community RustDesk-compatible remote desktop client for LG webOS TVs (webOS 5.x and newer). It
connects **out** to a computer running the normal RustDesk app (protocol baseline RustDesk 1.5.0).

**New name:** RemoteDesk TV is now **RustCouch**. It is the same app with the same app ID, so it
updates in place and keeps your computers, saved passwords and settings.

Tested on an **LG C3** with a Windows RustDesk host: connecting, Magic Remote pointer, clicks and
scrolling, remote buttons, hardware video, keyboard shrink and zoom work. Each build is also checked in
CI (cross-compile, ELF/ABI audit, emulated self-test, IPK verification). Please report problems.

### Install

* **Rooted TV / Homebrew Channel:** webOS Dev Manager, or `scp` + `luna-send` (see `docs/INSTALL_ROOTED.md`).
* **Developer Mode:** `ares-install --device tv io.github.danzig666.remotedesktv_<ver>_arm.ipk` (see `docs/INSTALL_DEVMODE.md`).

### What is in it

* ID connections through the public RustDesk server or self-hosted servers (separate profiles), and direct IP.
* Password login with optional "Remember password?", 2FA, waiting for remote approval.
* Hardware decoding (H.264, H.265, VP9, AV1) via the TV's Starfish media pipeline, so the picture can be shrunk and zoomed; NDL DirectMedia full-screen fallback; software VP9/VP8 fallback.
* Remote audio with mute.
* Magic Remote pointing; hold OK/wheel for right click; remote number buttons type digits; Red right click, Green next monitor, Yellow mouse/arrow mode, Blue keyboard, Back toolbar.
* Full on-screen keyboard: US and Hungarian layouts, Ctrl/Alt/AltGr/Shift/Win (once or locked), F-keys, numpad, per-computer shortcuts, Ctrl+Alt+Del.
* Fit, 1:1 and zoom; keyboard shrink or overlay.
* Default disconnect when leaving the app, with Reconnect; optional keep-connected; 30 s automatic reconnect.
* Nearby tab: RustDesk computers on the local network; Wake-on-LAN from a computer's ⋯ menu.
* Accounts and address book where the server provides them.
* USB/Bluetooth keyboards: shortcuts with held modifiers (Alt+Tab, Win+E, Ctrl+C) and special keys.

### Known limitations

* webOS keeps some keyboard function keys for the TV (on an LG C3: F2, F11, F12). Use the
  on-screen keyboard's **More** page for those.
* A keyboard's Esc arrives as the remote's Back; it is sent as Esc while you are typing.

### Files

* `io.github.danzig666.remotedesktv_<ver>_arm.ipk`: the app (licences and third-party notices are inside).
* `remotedesk-tv-<ver>-source.tar.gz`: complete corresponding source (AGPL-3.0), including the pinned RustDesk submodule.

Not affiliated with or endorsed by RustDesk or LG.

# TV verification checklist (non-root Developer Mode, webOS 5.x, and rooted TVs)

Use this on any TV, and especially on:

* a **genuinely non-rooted** TV in LG Developer Mode, since the owner only has rooted TVs, and
* a **webOS 5.x** TV, the oldest supported generation.

It takes about 30 minutes. You need the TV, a computer with RustDesk (Windows preferred), and the
test IPK from the Releases page.

## Record first

| Field | Value |
|---|---|
| TV model | |
| webOS version (Settings → General → About) | |
| Firmware version | |
| Rooted? (yes/no) / install route (Developer Mode, Homebrew, Dev Manager) | |
| App version | |
| Computer OS + RustDesk version | |
| Server (public / self-hosted OSS / Pro) | |

Then open **Settings → Diagnostics** in the app and photograph the screen (or save the report and
attach `data/diagnostics.txt`).

## Steps (mark ✅ / ❌ / notes)

1. Install ([INSTALL_DEVMODE.md](INSTALL_DEVMODE.md) or [INSTALL_ROOTED.md](INSTALL_ROOTED.md)). Install succeeded?
2. Launch from the **launcher**. Home screen appears? Text sharp and readable from the sofa?
3. Arrow buttons move the focus ring; OK activates; Magic Remote pointer focuses items.
4. Diagnostics: "NDL DirectMedia hardware decoder" says **available**? Which codecs are advertised?
5. Connect by **ID** through the public server with the password. Session starts? Toolbar shows "Direct (P2P)" or "Relayed"?
6. Picture appears. Toolbar → **Info**: codec, hardware or software decoder, frames per second.
7. Move a window on the computer: is motion smooth? Rough frame rate?
8. Magic Remote pointing lands exactly where the pointer is (check all four corners).
9. **Red** = right click, **Green** = next monitor (with 2 monitors), **Yellow** = Mouse/Arrow mode, **Blue** = keyboard, **Back** = toolbar (does not disconnect).
10. Hold an arrow button in Mouse mode: the mouse moves and accelerates. OK clicks.
11. Keyboard: type `Hello áéíóöőúüű ÁÉÍÓÖŐÚÜŰ` into Notepad with the HU layout. Exact result?
12. Ctrl then Shift then Esc opens Task Manager (Windows). Keys → **Send Ctrl+Alt+Del** shows the security screen.
13. Play a sound on the computer: is it heard on the TV? Toolbar **Mute** works?
14. Press **Home**, wait 5 s, return to the app: "Disconnected" with a **Reconnect** button?
15. Unplug the computer's network for 10 s during a session: countdown appears, reconnects after replugging?
16. USB keyboard (if available): typing works; Ctrl+Alt+Shift+Space opens the toolbar.
17. Settings → Hardware video decoding **off**, reconnect: picture still works (software decoding)? Frame rate?
18. Exit the app (Back on Home → Yes) and relaunch: settings and recent computers kept?

## Report

Open an issue on the repository with the table, the step results and the Diagnostics report. Never
include passwords. The report contains none, and the log never records typed text.

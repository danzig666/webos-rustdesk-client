# RemoteDesk TV user guide

RemoteDesk TV connects your LG TV to a computer running the normal **RustDesk** app (Windows,
Linux or macOS). You control the computer from the sofa. Nothing on the TV can be controlled from
the computer.

## On the computer

1. Install RustDesk from <https://rustdesk.com> (tested protocol baseline: 1.5.0).
2. Note the **ID** and **password** shown in RustDesk. A permanent password lets you use
   "Remember password?" on the TV.
3. Optional, for LAN connections without a server: RustDesk → Settings → Security →
   **Enable direct IP access** (port 21118). Then type the computer's IP address on the TV instead
   of the ID.

## First start

Choose **Use the RustDesk public server**, or **Add my self-hosted server** (ID server, optional
relay and API server, and the server **key** from `id_ed25519.pub`). The key lets the TV verify the
computers it talks to. Without it, connections still work but are not verified end to end.

## Home

* **Computer ID or address**: press OK and type with the on-screen or a USB keyboard, then **Connect**.
  Or just press the remote's **number buttons**: the ID field opens with the digits already entered,
  and OK (on **Done**) connects.
* **Favorites / Recent / Address book** tabs: OK connects; **⋯** opens options (favourite, rename,
  computer settings, forget saved password, remove from recent).
* **Server:** button: switch server profile. Each profile has its own favourites, history, saved
  passwords and account. The same ID on two servers shares nothing.
* **Account & address book**: sign in when your server provides accounts (RustDesk Pro or a
  compatible API server). ID and password always work without signing in.

## Password

Type the password and choose **Remember password?** if you want one-press connections next time.
Forget it later from the computer's ⋯ menu or computer settings. You can also leave the password
empty and wait for someone at the computer to click Accept.

## Buttons during a session

| Button | Action |
|---|---|
| Magic Remote pointer | Move the mouse; **OK/click** = left click; **hold OK ½ s** = right click; **wheel** = scroll |
| **Red** | Right click |
| **Green** | Next monitor (wraps around); the monitor name is shown |
| **Yellow** | Arrow buttons: **Mouse mode** ↔ **Arrow-key mode** |
| **Blue** | On-screen keyboard |
| **Back** (short press) | Closes the open menu or keyboard; otherwise shows/hides the toolbar. Never disconnects |
| Arrow buttons | Mouse mode: move the mouse (hold to accelerate); OK clicks. Arrow mode: arrow keys, OK = Enter |
| CH + / CH − | Scroll up / down |
| Number buttons 0–9 | Type digits on the computer (and digits into the password/code dialogs) |
| USB keyboard | Types on the computer; **Ctrl+Alt+Shift+Space** opens the toolbar |

No colour buttons? Everything is also in the **toolbar** (press Back): Keyboard, Monitor, Mouse/Arrows,
View, Keys, Mute, Quality, Info, Disconnect.

## Toolbar menus

* **View**: Fit to screen (default), Original size 1:1, Zoom in/out. When zoomed, move the Magic
  Remote pointer to a screen edge to scroll the view (the arrow buttons' mouse also drags it along).
  These only change the TV's view, never the computer's resolution. With hardware decoding this
  needs Settings → Hardware picture placement (on by default, experimental). If the TV cannot place
  the hardware picture, it always shows the full picture; Settings → Diagnostics says why.
* **Keys**: Ctrl+Alt+Del (the real secure-attention action, if the computer allows it), Lock remote
  computer, Escape, Windows key, Alt+Tab, Alt+F4, Double-click, **Drag lock** (the next OK presses
  and holds the left button until the following OK), Refresh video, Release stuck keys, Lock on
  disconnect, Computer settings.
* **Quality**: Balanced (default), Best (sharpest text), Low (fast), 30/60 fps.
* **Info**: codec, hardware/software decoder, frame rate, route (direct/relayed), encryption.

## On-screen keyboard

* **Ctrl, Alt, AltGr, Shift, Win** (Cmd/Option for Macs): select once = **once** (applies to the
  next key); select again = **locked**; again = off. Example: Ctrl, Shift, then Esc sends
  Ctrl+Shift+Esc.
* **EN→HU / HU→EN** switches between US English and Hungarian (QWERTZ with á é í ó ö ő ú ü ű, AltGr
  symbols). This only changes the TV keyboard; the computer's keyboard layout is not touched.
  Letters are typed as text, so they come out right whatever layout the computer uses.
* **More**: Esc, F1–F12, Print Screen, Scroll Lock, Pause, Insert/Home/End/Page Up/Page Down.
  **123**: numeric keypad.
* **Shortcut row** (saved per computer): defaults Alt+Tab, Task Manager, Win+D, Copy, Paste. Copy
  and paste act on the computer only; nothing is copied to or from the TV. Press keys, then
  **+ Save** to add the last key combination; **Remove…** deletes shortcuts.
* By default the picture **shrinks** above the keyboard so the whole screen stays visible; View →
  Keyboard switches to **overlay**. Closing the keyboard restores your previous view.

## Leaving the app and reconnecting

* Leaving the app (Home button, another app) **disconnects** after a moment. When you come back, a
  **Reconnect** button is shown.
* Settings → **Keep connected in background** keeps the session; input is paused, audio muted and
  video stopped until you return. The TV may still close the app in the background.
* If the connection drops unexpectedly, the app retries for up to **30 seconds** with a countdown
  and **Cancel**, then offers **Retry**. Wrong passwords, key problems and refusals are shown
  instead of retried.

## Settings

Server profiles, account, keep connected in background, hardware decoding, keyboard escape
shortcut, reduced motion, interface size, clear history, export settings (without passwords and
tokens), diagnostics, buttons & help, about & licences.

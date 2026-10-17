# Compatibility report

**Status: tested by the owner on an LG C3** (connecting, Magic Remote, remote buttons, hardware
video, keyboard shrink, zoom), with a Windows RustDesk host. Everything below is labelled with what
it actually rests on.

## Tested combinations

| Client build | Where it ran | Host | Server / route | Result |
|---|---|---|---|---|
| Desktop simulator (x86_64, SDL2 X11, software decode) | Claude Code Cloud container, Xvfb 1920×1080 | Stock RustDesk **1.5.0** `.deb`, Linux/X11 (Xvfb), unmodified | direct IP `127.0.0.1:21118` | Connected, password login, live 1920×1080 VP9 picture, toolbar, OSK in shrink mode, mouse click and text delivered |
| Protocol probe, x86_64 | same | same | same | VP9 1080p decoded, 0 drops, cursor, input, close reason |
| Protocol probe, **ARM (webOS toolchain build)** under qemu-arm, Debian armel glibc 2.36 userland | same | same | same | Connected, logged in, NEON libvpx decoded 1080p VP9 (35/35 frames), input delivered |
| `remotedesk-tv --self-test`, ARM under qemu (armel) | container and GitHub Actions | — | — | libvpx, libopus, libsodium, key decoding, storage: PASS |
| IPK + ELF symbol check: `webosbrew-ipk-verify` 0.9.0 against firmware symbol dumps | GitHub Actions (run 1) | — | — | **All OK** for webOS **5.3.1, 6.4.0, 7.4.0, 8.3.0, 9.2.0, 10.2.0, 11.2.0** (binary and bundled `libSDL2-2.0.so.0`). Every imported library and symbol exists on those firmwares (static check, not a launch) |

Evidence: [evidence/2026-10-17-linux-host-probe.md](evidence/2026-10-17-linux-host-probe.md).

## Not yet tested (awaiting hardware or access)

| Item | Needed | Why it is not done here |
|---|---|---|
| **LG C3, webOS 10.2.2, fw 33.23.05, O22N (rooted)**: launch, render, keys, audio, NDL | Owner's TV | No TV access from the cloud environment |
| Any **webOS 5.x** TV | A webOS 5 tester | Required before claiming oldest-version support |
| **Developer Mode (non-rooted)** install and NDL access | A genuinely non-rooted TV | Owner has rooted TVs only. A rooted TV launched without sudo does not count |
| **Windows 11 + RustDesk 1.5.0** host (first interop target): UAC, sign-in screen, Ctrl+Alt+Del, H.264/H.265 hardware encoding | Owner's PC | Only a Linux host was available here |
| macOS host | a Mac | not available |
| ID connection through public / self-hosted rendezvous (P2P punch and relay) | outbound TCP 21116/21117 | the container's egress only allows HTTPS proxying |
| Accounts / address book | legitimate account on a supporting API server | none available |
| Audio | host with an audio device | the test host had no PulseAudio |
| 1080p60 / 4K60 performance, 8-hour soak, 50 connect cycles | TV + Windows host | — |

## Known limitations (with evidence)

1. **Linux/X11 hosts drop the case of some upper-case accented letters** typed as text
   (`Á É Í Ó Ö Ú Ü` arrive lower case; `Ő Ű` are correct). It is the same with `seq`, `unicode` and
   `chr` key events, with or without Shift, so it is host-side text injection (enigo/xdo on X11), not
   this client. The Linux host also needs a UTF-8 locale, otherwise xdo rejects all non-ASCII.
   Windows hosts use a different (Unicode SendInput) path: **to be verified**.
2. **Hardware decoding presents full-screen.** NDL DirectMedia API 2 has no destination rectangle,
   so with hardware decoding the view is always "Fit" and the on-screen keyboard overlays the
   picture. 1:1, zoom and keyboard-shrink are available with software decoding (Settings → Hardware
   video decoding off). Fit placement and letterboxing of the NDL plane are **unverified** (assumed
   aspect-preserving, centred).
3. **AV1 is advertised only with hardware decoding on webOS ≥ 6.** There is no AV1 software
   decoder. If the TV refuses AV1, the client withdraws it and the host switches codec
   (re-advertisement path unit-tested, not tested on a TV).
4. **D-pad arrows vs keyboard arrows:** SDL2 cannot tell remote and keyboard arrows apart. Typing on
   a physical keyboard switches arrows/Enter to key mode for 10 s; Yellow switches permanently.
5. **Direct IP connections are not end-to-end verified**, exactly as upstream: there is no
   rendezvous-signed peer key. The toolbar shows "encrypted" only when a key was negotiated.
6. **Physical keyboards with non-US layouts** produce characters from SDL keycodes (US shift map
   for ASCII). For Hungarian text, the on-screen keyboard's HU layout is the verified path.

## Release gates (stable release)

A stable (non-pre-release) build needs real evidence for:

1. Launcher start, relaunch and exit on the owner's C3 (rooted) **and** on a genuinely non-rooted
   Developer Mode TV.
2. Picture, audio and input on Windows 11 / RustDesk 1.5.0 through the public server and through a
   self-hosted server (P2P and relay).
3. Hardware decode (H.264 or H.265) at 1080p60 with measured frame delivery.
4. Lifecycle: default disconnect + Reconnect, keep-in-background, 30 s reconnect.
5. A webOS 5.x launch + decoder probe, or webOS 5 declared unverified in the release notes.

These gates are tracked here; releases are no longer marked as pre-releases (D25).

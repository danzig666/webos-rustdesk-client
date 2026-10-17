# Interop probe against stock RustDesk 1.5.0 (Linux/X11 host), 2026-10-17

Environment: Claude Code Cloud container (x86_64). Host = official
`rustdesk-1.5.0-x86_64.deb`, unmodified, running `rustdesk --server` on Xvfb 1920x1080,
options `direct-server=Y`, `approve-mode=password`, permanent password. Client = this
repository's `rdtv-core` + `rdtv-media` via `cargo run -p rdtv-media --example probe --
127.0.0.1 <password> 7` (desktop build, software decode; NDL absent on x86).

Evidence class: **tested locally** (not on a TV).

| Check | Result |
|---|---|
| Direct-address connection (`127.0.0.1:21118`) | OK, route "Direct (address)", handshake unsecured (expected: no rendezvous-signed key for direct IP, same as upstream) |
| Password login (salted challenge) | OK; `LoginResponse.PeerInfo` received (host `Linux`, version `1.5.0`, one 1920x1080 display) |
| Login option message | Host log shows `disable_clipboard: Yes, enable_file_transfer: No`, `ability_vp9: 1, ability_vp8: 1, h264/h265/av1: 0` |
| Codec negotiation | Host created `VpxEncoderConfig { codec: VP9, 1920x1080 }` |
| Video | 18-30 VP9 frames decoded by libvpx in 6-10 s on a mostly static desktop (frames are only sent on change), 0 dropped, picture 1920x1080 |
| Cursor | `CursorData` 16x16 accepted and validated |
| Mouse | Absolute move + left click focused an xterm at (1200,650) |
| Text | `echo 'Hello áéíóöőúüű ...' > /tmp/rdtv_typed.txt` + Enter executed on the host |
| Hungarian lower case | `áéíóöőúüű` correct (after the host process had a UTF-8 locale; with `LANG` unset the host's xdo rejects all non-ASCII: host environment issue) |
| Hungarian upper case | `Ő Ű` correct; `Á É Í Ó Ö Ú Ü` arrive lower-case. Same result via `seq`, `unicode` and `chr` key events, with or without Shift: host-side X11 text injection (enigo/xdo) limitation, not client-side |
| Disconnect | Close reason sent, host log shows the input thread exiting |

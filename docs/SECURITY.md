# Security and storage behaviour

## What is stored, and where

All app data lives in the app's private data directory:

- **On the TV:** `$HOME/data`. The launcher sets `HOME` to the app directory.
- **On a desktop build:** `~/.local/share/remotedesk-tv`.
- **Override:** the `RDTV_DATA_DIR` environment variable.

Files are written atomically (temporary file, then rename) with mode **0600**.

| File | Contents | Secrets |
|---|---|---|
| `state.json` | Server profiles, per-profile favourites, recents and per-computer settings; app settings; local client ID and UUID; cached address book (IDs, names, tags only) | none |
| `secrets.json` | Per profile and per peer: the remembered password hash. Per profile: the API access token. | yes |
| `logs/remotedesk-tv.log` (+ `.1`) | Operational log | none by design (see below) |
| `upstream/` and `~/.config/RemoteDeskTV` | Upstream `hbb_common` config (socks proxy lookup, local key) | none |

**Isolation:** every record is keyed by server profile first. The same peer ID under two profiles shares no password, setting, history or address-book entry. A unit test covers this (`store::tests::profile_isolation_and_roundtrip`).

## Remembered passwords

This follows the owner's decision: a simple "Remember password?" choice, with no PIN, no master password and no local lock.

- **What is stored:** the upstream RustDesk salted hash `sha256(password || salt)`, using the host's salt. The plaintext password is never written to disk, and it stays in memory only while a login is in progress.
- **What this is not:** encryption. Anyone with filesystem or root access to the TV can read `secrets.json` and replay the hash against that host. Do not treat the TV as a secure credential store.
- **Reconnects:** the hash of the current session is kept in memory only, so an automatic reconnect does not ask again. It is persisted only after the user opts in.
- **Removing it:**
  - "Forget saved password" (per computer).
  - "Clear history" (per profile; favourites are kept).
  - Deleting a profile removes all of its secrets.
  - "Sign out" removes the profile's token and cached address book.
- **Wrong password:** a remembered hash that the host rejects is deleted automatically.

## Logs, diagnostics, export

- Logs never contain passwords, hashes, tokens or typed text. Remote clipboard and screen contents are never logged, and clipboard is disabled anyway. Third-party info-level logging is filtered out.
- The diagnostics screen and "Save diagnostic report" write only to the data directory. Nothing is uploaded automatically, and there is no telemetry.
- "Export settings" excludes `secrets.json` content. Secrets are included only by an explicit API flag, which is not exposed in the UI.

## Trust and transport verification

These rules are unchanged from upstream `Client::secure_connection`:

- **ID connections:**
  - The rendezvous server returns the peer's signed public key. When that signature verifies with the profile's key (the public server's `RS_PUB_KEY`, or the configured self-hosted key), the client checks the host's `SignedId` and negotiates a session key. The session is then end-to-end encrypted, and the UI shows "encrypted".
  - If verification is impossible (no key, or a key mismatch), the client falls back exactly as upstream does, and the UI does **not** claim encryption.
  - Changed keys are never auto-trusted beyond what upstream does.
- **Direct-address connections:** there is no rendezvous-signed key, so the handshake is unverified, exactly as upstream. The host decides whether direct access is allowed.
- **Account API:** HTTPS uses rustls with the bundled webpki roots, and certificates are always verified. Plain `http://` API URLs are only used if the user configures them.

## Malformed input defences

- **Cursor images:** capped at 256×256, and the RGBA length is validated after optional zstd decompression.
- **Video and audio:** compressed frames over 64 MB are rejected before decoding. Decoded geometry is checked (≤ 8192 px, sane strides). The decode queue is bounded at 120 frames; overflow drops frames and requests a keyframe instead of growing. The audio queue is capped at 250 ms.
- **HTTP responses:** capped at 8 MB with a 15 s timeout. Chunked decoding is length-checked.
- **Imports and files:** settings files over 4 MB are ignored. Server config strings over 4096 characters are rejected. Text fields are capped at 512 characters.
- **NDL:** the library is resolved with `dlopen`/`dlsym`. A missing library or symbol becomes a capability error, not a crash, and calls are serialized by a mutex.
- **Stale events:** session events carry a generation ID, and events from an old session are dropped.

## Excluded features are disabled, not hidden

- The login option message sets `disable_clipboard = Yes` and `enable_file_transfer = No` (verified in the host log during the [interop probe](evidence/2026-10-17-linux-host-probe.md)).
- Incoming clipboard, file, chat, voice, terminal, screenshot and port-forward messages are ignored without side effects.
- There is no code path that reads or writes the TV clipboard.

## Privileges and network exposure

- There is no inbound listener of any kind, and no service or helper.
- The app runs as the normal launcher-started user and needs no root. It never modifies the TV's jail, firmware, picture or audio settings, or other apps.
- On a rooted TV the app still runs unprivileged when launched normally. The diagnostics screen reports the uid.

## Reporting issues

Open a GitHub issue. For anything sensitive, contact the repository owner privately rather than posting details publicly.

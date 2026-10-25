# License and naming review

This is an engineering review, not legal advice. The final product name has not been legally cleared.

## Project license

RustCouch is licensed under **AGPL-3.0-only** (`LICENSE`). The reason is that it links the AGPL-3.0 RustDesk crates `hbb_common` and `base` unmodified, and it ports AGPL-3.0 code from RustDesk's `src/client.rs`, `src/common.rs` and `src/client/io_loop.rs`. That upstream code is pinned as the git submodule `third_party/rustdesk` at `fada664` (tag 1.5.0). Each ported module states its origin.

## Components and compatibility

| Component | How it is used | License | Compatible with AGPL-3.0 | Notes |
|---|---|---|---|---|
| RustDesk `hbb_common`, `base`, ported client code | linked and ported | AGPL-3.0 | yes (same license) | Copyright notices are preserved in module headers |
| SDL2 webOS backport (webosbrew) | shared library in `lib/` | Zlib | yes | Licence text in `licenses/ALL-LICENSES.txt` (section `SDL2/`) |
| libvpx 1.15.2 | static | BSD-3-Clause + patent grant (`PATENTS`) | yes | `licenses/ALL-LICENSES.txt` (sections `libvpx/LICENSE`, `libvpx/PATENTS`) |
| libopus 1.5.2 | static | BSD-3-Clause | yes | `licenses/ALL-LICENSES.txt` (section `opus/COPYING`) |
| OpenSSL 3.x (`openssl-src`, vendored) | static, pulled in by upstream `native-tls` | Apache-2.0 | yes (Apache-2.0 is GPLv3-compatible) | License files shipped from the crate |
| libsodium (`libsodium-sys`) | static | ISC | yes | `licenses/ALL-LICENSES.txt` (sections `crates/libsodium-sys-*`) |
| Inter 4.1 (Regular, SemiBold), interface font | embedded | SIL Open Font License 1.1 | yes | `licenses/LICENSE-Inter.txt`. Unmodified font files, bundled with software (allowed by OFL), not sold on their own, name not used for a modified version. |
| DejaVu Sans fonts (symbol fallback) | embedded | Bitstream Vera license + public domain changes | yes | `licenses/LICENSE-DejaVu.txt`. Font name and redistribution terms are respected; the fonts are not renamed. |
| fontdue, rustls, ring, tokio, protobuf and the other ~250 Rust crates | static | MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode / MPL-2.0 (per crate) | yes | `THIRD-PARTY-NOTICES.txt` plus every crate's LICENSE, COPYING and NOTICE text in `licenses/ALL-LICENSES.txt`, one section per file (collected by `scripts/gen-notices.py`). An SBOM (`sbom.cdx.json`) is part of each release build's workflow artifact. |
| punktfunk/client-webos | approach only: toolchain flags, glibc shim idea, NDL dlopen/state handling | MIT OR Apache-2.0 | yes | Re-implemented with attribution in source comments and `SOURCES.md`; no files were copied |
| mariotaku/ss4s | approach only: video-only NDL load, ms PTS | LGPL-3.0 | n/a (no code used) | Re-implemented independently in Rust |
| webosbrew/webos-userland | struct layouts and symbol names as reference | Unlicense (public domain) | yes | |
| moonlight-tv | build and packaging approach only | GPL-3.0 | n/a (no code used) | |
| LG NDL DirectMedia (`libNDL_directmedia.so.1`) | opened with `dlopen` from the TV at runtime | proprietary (LG) | not redistributed | Never shipped, never linked, and no SDK stub is bundled. `scripts/elf-audit.sh` fails the build on a DT_NEEDED for it. |

## Corresponding source

The AGPL and LGPL-style obligations are met as follows:

- **Public repository:** https://github.com/danzig666/webos-rustdesk-client, with pinned submodules, `Cargo.lock`, `rust-toolchain.toml`, and the SDK URL and hash in `.github/workflows/build.yml`.
- **Per-release source archive:** every release attaches `remotedesk-tv-<version>-source.tar.gz`, built with `git ls-files --recurse-submodules` so the RustDesk sources are included.
- **C libraries:** `scripts/build-deps.sh` rebuilds the static libraries from checksummed or commit-verified sources.
- **Network use:** AGPL §13 (users interacting over a network) does not apply in practice here. The app is a client the user runs locally, and the source offer above is provided anyway. The About screen links the source.

## Naming and branding

- **"RustCouch"** (since 0.4.2; formerly the working name "RemoteDesk TV") hints at RustDesk compatibility and at the sofa, without using the RustDesk name or anything resembling it ("Desk"). Compatibility is stated descriptively ("RustDesk-compatible", "for RustDesk"). "Rust" is also the name of the programming language (Rust Foundation trademark); this app is written in Rust and does not claim any endorsement. The name has not been searched or cleared formally.
- **RustDesk name:** the pinned RustDesk repository has no trademark or naming policy file (its `README.md` and docs were searched for "trademark"; only translation guidance in `AGENTS.md` mentions the brand as a token). The product is therefore described factually as a "RustDesk-compatible client", with an explicit statement that it is not made or endorsed by RustDesk / Purslane Ltd.
- **Logos:** no RustDesk, LG, webOS, Plex or other project logos are used.
- **Icon:** the owner supplied the icon artwork. `packaging/webos/icon.svg` is a vector recreation of it, and `scripts/render-icons.py` renders the PNG sizes.
- **App ID:** `io.github.danzig666.remotedesktv` sits in the repository owner's GitHub namespace. Keep it unchanged for upgrades.

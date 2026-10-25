# Submitting RustCouch to the official webosbrew catalog

`io.github.danzig666.remotedesktv.yml` is the package file for
[webosbrew/apps-repo](https://github.com/webosbrew/apps-repo) (repo.webosbrew.org, the Homebrew
Channel's default repository). It passes their linter (`python -m repogen.lintpkg -f
packages/io.github.danzig666.remotedesktv.yml --new`: "Check passed").

Its `manifestUrl` points at the project's own catalog branch, which every release rewrites, so
once merged the official catalog follows new releases without further pull requests.

## Steps

1. Fork https://github.com/webosbrew/apps-repo on GitHub.
2. Add this file to the fork as `packages/io.github.danzig666.remotedesktv.yml`.
3. Open a pull request with the text below (edit it to match your own words and testing).

The submitter is responsible for the code and must disclose AI use (their rule 1, "NO AI SLOP":
apps made primarily by AI and submitted without meaningful human development, testing and review
are rejected). Be accurate about how the app was made and tested; the maintainers decide.

## Pull request text (draft)

```markdown
Add RustCouch (io.github.danzig666.remotedesktv)

RustCouch is an open-source (AGPL-3.0) RustDesk-compatible remote desktop client for webOS 5+.
It connects out to a computer running the stock RustDesk app (public server, self-hosted server
or direct IP) and is controlled with the Magic Remote, the remote's buttons, an on-screen keyboard
or a USB/Bluetooth keyboard. No root required; no inbound listener.

Source: https://github.com/danzig666/webos-rustdesk-client
Releases are built in CI (cross-compiled with the webOS NDK, `webosbrew-ipk-verify --fw-releases
'>=5'` must pass), and the manifest is regenerated on each release.

**AI disclosure:** this app was written with substantial help from an AI coding assistant
(Claude Code). I directed the design, reviewed the changes and tested every build on my own
LG C3 over many iterations (connection, Magic Remote input, remote buttons, number keys, hardware
decoding via the Starfish pipeline, zoom/keyboard shrink, Bluetooth keyboard shortcuts), reporting
bugs back until they were fixed. Testing so far: LG C3 (webOS 23) with a Windows 11 RustDesk 1.5.0
host. Other models are untested.
```

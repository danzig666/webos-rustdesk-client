# Building

GitHub Actions (`.github/workflows/build.yml`) is the reference build: every push produces the IPK
as a workflow artifact, and `v*` tags publish a pre-release. A local Linux build is optional
(Windows users can use WSL2 Ubuntu 24.04).

## Pinned inputs

| Input | Pin |
|---|---|
| Rust | 1.97.0 (`rust-toolchain.toml`), target `armv7-unknown-linux-gnueabi` |
| RustDesk | submodule `third_party/rustdesk` at 1.5.0 `fada664`, `libs/hbb_common` `229b9045` |
| webOS SDK | openlgtv/buildroot-nc4 `webos-a38c582` x86_64, sha256 `04ad3311…3616` (gcc 14.2, glibc 2.12 sysroot) |
| SDL2 | webosbrew/SDL-webOS `release-2.30.12-webos.6` (upstream ABI), sha256 `63dc9ea2…c698` |
| libvpx | 1.15.2, commit `d168454e` |
| libopus | 1.5.2, sha256 `65c1d2f7…7ce1` |
| Rust crates | `Cargo.lock` (`--locked` in CI) |
| GitHub Actions | pinned by commit SHA |

## Local build (Linux x86_64)

```sh
git clone --recurse-submodules https://github.com/danzig666/webos-rustdesk-client
cd webos-rustdesk-client
sudo apt-get install -y build-essential nasm pkg-config libsdl2-dev qemu-user python3

# SDK
curl -fsSLO https://github.com/openlgtv/buildroot-nc4/releases/download/webos-a38c582/arm-webos-linux-gnueabi_sdk-buildroot-x86_64.tar.gz
echo "04ad3311b48b4557a7002aef56ae2e167478e8e129f37daac04649bddf813616  arm-webos-linux-gnueabi_sdk-buildroot-x86_64.tar.gz" | sha256sum -c
tar xzf arm-webos-linux-gnueabi_sdk-buildroot-x86_64.tar.gz -C /opt
/opt/arm-webos-linux-gnueabi_sdk-buildroot/relocate-sdk.sh
export WEBOS_SDK=/opt/arm-webos-linux-gnueabi_sdk-buildroot

# TV build
scripts/build-deps.sh webos
cargo build --locked --release --target armv7-unknown-linux-gnueabi -p rdtv-app
scripts/elf-audit.sh target/armv7-unknown-linux-gnueabi/release/remotedesk-tv deps-out/webos/lib/libSDL2-2.0.so.0
scripts/smoke-armel.sh target/armv7-unknown-linux-gnueabi/release/remotedesk-tv deps-out/webos/lib/libSDL2-2.0.so.0
python3 scripts/gen-notices.py --licenses-dir build/licenses && cp -r deps-out/webos/licenses/* build/licenses/
python3 scripts/package-ipk.py --binary target/armv7-unknown-linux-gnueabi/release/remotedesk-tv \
  --sdl-lib deps-out/webos/lib/libSDL2-2.0.so.0 --version 0.1.0 \
  --strip $WEBOS_SDK/bin/arm-webos-linux-gnueabi-strip --licenses build/licenses --out dist
```

## Desktop simulator

Same UI and state logic, with SDL2 on X11/Wayland and software decoding. It cannot show TV
hardware decoding or TV performance.

```sh
scripts/build-deps.sh host
cargo run -p rdtv-app            # F1-F4 = Red/Green/Yellow/Blue, F9 = Back
cargo test -p rdtv-core -p rdtv-media -p rdtv-app
```

## Releases

* Push a tag `vX.Y.Z`, or run the workflow manually with `release_tag`. The release carries only
  the IPK (licences and notices inside) and the complete source archive (with submodules). The
  SBOM, checksums, ELF audit, smoke test output and Homebrew manifest stay in the workflow artifact.
* The webOS package version is the numeric `X.Y.Z` part of the tag.

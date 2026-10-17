#!/usr/bin/env bash
# Build the static C media libraries the client links: libvpx (VP9/VP8 software fallback)
# and libopus (audio). Pinned source revisions, verified by SHA-256.
#
#   scripts/build-deps.sh webos   # cross build for the TV (needs WEBOS_SDK)
#   scripts/build-deps.sh host    # native build for the desktop simulator / tests
#
# Output: deps-out/<target>/{include,lib}
set -euo pipefail
TARGET="${1:?usage: build-deps.sh webos|host}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/deps-out/$TARGET"
SRC="$ROOT/build/deps-src"
JOBS="$(nproc 2>/dev/null || echo 4)"

VPX_VER=1.15.2
VPX_COMMIT=d168454ecd099805c675d4a98c66f4891373302a
OPUS_VER=1.5.2
OPUS_SHA=65c1d2f78b9f2fb20082c38cbe47c951ad5839345876e46941612ee87f9a7ce1

mkdir -p "$SRC" "$OUT"
fetch() { # url sha file
  if [ ! -f "$SRC/$3" ]; then curl -fsSL -o "$SRC/$3.part" "$1" && mv "$SRC/$3.part" "$SRC/$3"; fi
  echo "$2  $SRC/$3" | sha256sum -c - >/dev/null || { echo "checksum mismatch for $3" >&2; exit 1; }
}
if [ ! -d "$SRC/libvpx-$VPX_VER/.git" ]; then
  rm -rf "$SRC/libvpx-$VPX_VER"
  git clone -q --depth 1 --branch "v$VPX_VER" https://github.com/webmproject/libvpx "$SRC/libvpx-$VPX_VER"
fi
[ "$(git -C "$SRC/libvpx-$VPX_VER" rev-parse HEAD)" = "$VPX_COMMIT" ] || { echo "libvpx commit mismatch" >&2; exit 1; }
fetch "https://downloads.xiph.org/releases/opus/opus-$OPUS_VER.tar.gz" "$OPUS_SHA" "opus-$OPUS_VER.tar.gz"

case "$TARGET" in
  webos)
    : "${WEBOS_SDK:?WEBOS_SDK must point to arm-webos-linux-gnueabi_sdk-buildroot}"
    SYSROOT="$WEBOS_SDK/arm-webos-linux-gnueabi/sysroot"
    export CC="$WEBOS_SDK/bin/arm-webos-linux-gnueabi-gcc --sysroot=$SYSROOT"
    export CXX="$WEBOS_SDK/bin/arm-webos-linux-gnueabi-g++ --sysroot=$SYSROOT"
    export AR="$WEBOS_SDK/bin/arm-webos-linux-gnueabi-ar"
    export STRIP="$WEBOS_SDK/bin/arm-webos-linux-gnueabi-strip"
    export LD="$CC"
    # SDK float ABI is softfp; NEON is available on every webOS 5+ SoC.
    export CFLAGS="-O2 -fPIC -march=armv7-a -mfpu=neon -mfloat-abi=softfp"
    export CXXFLAGS="$CFLAGS"
    VPX_TARGET=armv7-linux-gcc
    OPUS_HOST=--host=arm-linux-gnueabi
    ;;
  host)
    export CFLAGS="-O2 -fPIC"
    VPX_TARGET=generic-gnu
    if [ "$(uname -m)" = x86_64 ]; then VPX_TARGET=x86_64-linux-gcc; fi
    OPUS_HOST=
    ;;
  *) echo "unknown target $TARGET" >&2; exit 2 ;;
esac

if [ ! -f "$OUT/lib/libvpx.a" ]; then
  rm -rf "$SRC/vpx-build-$TARGET" && mkdir -p "$SRC/vpx-build-$TARGET"
  git -C "$SRC/libvpx-$VPX_VER" archive HEAD | tar x -C "$SRC/vpx-build-$TARGET"
  (
    cd "$SRC/vpx-build-$TARGET"
    extra=()
    if [ "$TARGET" = webos ]; then
      extra+=(--disable-runtime-cpu-detect --enable-neon)
      # libvpx derives its tools from CROSS; the sysroot goes through the flags.
      export CROSS="$WEBOS_SDK/bin/arm-webos-linux-gnueabi-"
      export CFLAGS="--sysroot=$SYSROOT $CFLAGS" CXXFLAGS="--sysroot=$SYSROOT $CXXFLAGS" LDFLAGS="--sysroot=$SYSROOT"
      unset CC CXX AR LD STRIP
    fi
    if [ "$VPX_TARGET" = x86_64-linux-gcc ] && ! command -v nasm >/dev/null && ! command -v yasm >/dev/null; then
      extra+=(--disable-x86-asm)
    fi
    ./configure --target="$VPX_TARGET" --prefix="$OUT" \
      --disable-vp8-encoder --disable-vp9-encoder --enable-vp8-decoder --enable-vp9-decoder \
      --disable-examples --disable-tools --disable-docs --disable-unit-tests \
      --disable-webm-io --disable-libyuv --enable-pic --enable-static --disable-shared \
      --enable-multithread --enable-realtime-only "${extra[@]}" >/dev/null
    make -j"$JOBS" >/dev/null
    make install >/dev/null
  )
fi

if [ ! -f "$OUT/lib/libopus.a" ]; then
  rm -rf "$SRC/opus-build-$TARGET" && mkdir -p "$SRC/opus-build-$TARGET"
  tar xzf "$SRC/opus-$OPUS_VER.tar.gz" -C "$SRC/opus-build-$TARGET" --strip-components=1
  (
    cd "$SRC/opus-build-$TARGET"
    # shellcheck disable=SC2086
    ./configure $OPUS_HOST --prefix="$OUT" --enable-static --disable-shared --with-pic \
      --disable-doc --disable-extra-programs >/dev/null
    make -j"$JOBS" >/dev/null
    make install >/dev/null
  )
fi
# SDL2 with webOS integration (webosbrew/SDL-webOS backport, upstream struct ABI), bundled
# into the IPK under lib/ because the TV's own SDL2 has LG's modified event ABI.
if [ "$TARGET" = webos ] && [ ! -f "$OUT/lib/libSDL2-2.0.so.0" ]; then
  SDL_TAG=release-2.30.12-webos.6
  SDL_SHA=63dc9ea268bdf64801fce5e0c19585ee6aaf79f0b08486c853c47f922385c698
  fetch "https://github.com/webosbrew/SDL-webOS/releases/download/$SDL_TAG/SDL2-2.30.12-webos.tar.gz" "$SDL_SHA" "SDL2-2.30.12-webos.tar.gz"
  mkdir -p "$OUT/sdl2" && tar xzf "$SRC/SDL2-2.30.12-webos.tar.gz" -C "$OUT/sdl2"
  cp -a "$OUT/sdl2/lib/libSDL2-2.0.so"* "$OUT/sdl2/lib/libSDL2.so" "$OUT/lib/"
  mkdir -p "$OUT/include" && cp -a "$OUT/sdl2/include/SDL2" "$OUT/include/"
  mkdir -p "$OUT/licenses" && cp -a "$OUT/sdl2/share/licenses/SDL2" "$OUT/licenses/" 2>/dev/null || true
fi
echo "deps ready in $OUT"
ls -la "$OUT/lib"/

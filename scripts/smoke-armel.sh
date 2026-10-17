#!/usr/bin/env bash
# Run the TV binary's --self-test under qemu-arm on a Debian armel userland (glibc 2.36,
# soft-float calling convention like webOS). The SDK's own glibc 2.12 runtime cannot run
# under qemu-user, and real TVs ship glibc >= 2.28, so this is the closer stand-in.
# This is an emulation smoke test, not evidence of on-TV behaviour.
set -euo pipefail
BIN="${1:?binary}"
SDL="${2:?libSDL2-2.0.so.0}"
WORK="${SMOKE_DIR:-build/armel-root}"
mkdir -p "$WORK"
LIBC_DEB=libc6_2.36-9+deb12u14_armel.deb
LIBC_SHA=5eb674d8a73cccad372a7d35878209fc403266da1c23eb5974cdadb69de4091e
GCC_DEB=libgcc-s1_12.2.0-14+deb12u1_armel.deb
GCC_SHA=4d9bf44b70e4437e652160ec8c042ed7a186662281f4d58a728f4a87225f1527
fetch() { # dir file sha
  if [ ! -f "$WORK/$2" ]; then
    curl -fsSL -o "$WORK/$2" "https://deb.debian.org/debian/pool/main/$1/$2" ||
      curl -fsSL -o "$WORK/$2" "https://snapshot.debian.org/archive/debian/20261015T000000Z/pool/main/$1/$2"
  fi
  echo "$3  $WORK/$2" | sha256sum -c - >/dev/null
}
fetch g/glibc "$LIBC_DEB" "$LIBC_SHA"
fetch g/gcc-12 "$GCC_DEB" "$GCC_SHA"
ROOT="$WORK/root"
if [ ! -e "$ROOT/lib/ld-linux.so.3" ]; then
  mkdir -p "$ROOT/lib"
  dpkg-deb -x "$WORK/$LIBC_DEB" "$ROOT"
  dpkg-deb -x "$WORK/$GCC_DEB" "$ROOT"
  ln -sf arm-linux-gnueabi/ld-linux.so.3 "$ROOT/lib/ld-linux.so.3"
fi
cp "$SDL" "$ROOT/lib/arm-linux-gnueabi/libSDL2-2.0.so.0"
qemu-arm -L "$ROOT" -E LD_LIBRARY_PATH=/lib/arm-linux-gnueabi "$BIN" --self-test

#!/usr/bin/env bash
# ELF/ABI audit for the TV binary and bundled libraries. Fails on anything a webOS 5+
# TV could not load. Usage: elf-audit.sh <binary> [bundled libs...]
set -euo pipefail
: "${WEBOS_SDK:?WEBOS_SDK must point to the webOS SDK}"
RE="$WEBOS_SDK/bin/arm-webos-linux-gnueabi-readelf"
fail=0
err() { echo "FAIL: $*"; fail=1; }
# webOS 5 ships glibc 2.28 and libstdc++ 6.0.25 (GLIBCXX_3.4.25): the oldest supported target.
MAX_GLIBC=2.28
MAX_GLIBCXX=3.4.25
ver_gt() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" = "$1" ] && [ "$1" != "$2" ]; }
for f in "$@"; do
  echo "== $f"
  hdr="$($RE -h "$f")"
  echo "$hdr" | grep -q "ELF32" || err "$f: not ELF32"
  echo "$hdr" | grep -q "Machine:.*ARM" || err "$f: not ARM"
  echo "$hdr" | grep -q "soft-float ABI" || err "$f: not the softfp/soft-float calling convention"
  if $RE -A "$f" | grep -q "Tag_ABI_VFP_args: VFP registers"; then err "$f: uses the hard-float calling convention"; fi
  dyn="$($RE -d "$f")"
  if [ "$f" = "$1" ]; then
    $RE -l "$f" | grep -q "Requesting program interpreter: /lib/ld-linux.so.3" || err "$f: unexpected interpreter"
    echo "$dyn" | grep -q '(RPATH).*\[\$ORIGIN/../lib\]' || err "$f: DT_RPATH must be \$ORIGIN/../lib (RUNPATH loses to the launcher's LD_LIBRARY_PATH)"
    if echo "$dyn" | grep -q '(RUNPATH)'; then err "$f: has DT_RUNPATH; link with --disable-new-dtags"; fi
    for n in $(echo "$dyn" | sed -n 's/.*NEEDED.*\[\(.*\)\]/\1/p'); do
      case "$n" in
        libSDL2-2.0.so.0|libc.so.6|libm.so.6|libpthread.so.0|libdl.so.2|librt.so.1|libgcc_s.so.1|ld-linux.so.3) echo "  needed: $n" ;;
        *) err "$f: unexpected NEEDED $n (would have to exist on every TV)";;
      esac
    done
  else
    echo "$dyn" | sed -n 's/.*NEEDED.*\[\(.*\)\]/  needed (system): \1/p'
  fi
  if echo "$dyn" | grep -E 'R(UN)?PATH' | grep -qE '/home|/opt|/usr/lib/x86|/tmp|build'; then err "$f: host path in RPATH"; fi
  maxg="$($RE --dyn-syms -W "$f" | { grep -oE '@GLIBC_[0-9.]+' || true; } | sed 's/@GLIBC_//' | sort -V | tail -1 || true)"
  echo "  max GLIBC symbol: ${maxg:-none}"
  if [ -n "$maxg" ] && ver_gt "$maxg" "$MAX_GLIBC"; then err "$f: needs GLIBC_$maxg > $MAX_GLIBC"; fi
  maxx="$($RE --dyn-syms -W "$f" | { grep -oE '@GLIBCXX_[0-9.]+' || true; } | sed 's/@GLIBCXX_//' | sort -V | tail -1 || true)"
  if [ -n "$maxx" ]; then
    echo "  max GLIBCXX symbol: $maxx"
    if ver_gt "$maxx" "$MAX_GLIBCXX"; then err "$f: needs GLIBCXX_$maxx > $MAX_GLIBCXX"; fi
  fi
  if $RE -d "$f" | grep -q "libNDL_directmedia"; then err "$f: NDL must be dlopen()ed, not linked"; fi
done
[ $fail -eq 0 ] && echo "ELF audit passed" || { echo "ELF audit FAILED"; exit 1; }

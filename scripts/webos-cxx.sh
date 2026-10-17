#!/bin/sh
# Cross C++ compiler wrapper for the webOS (arm-webos-linux-gnueabi, softfp) SDK.
# The SDK's baked-in sysroot is wrong after relocation, so pass it explicitly.
# libstdc++ is linked statically so a bundled libstdc++.so never shadows the TV's newer one
# (approach documented by punktfunk/client-webos docs/NOTES.md).
: "${WEBOS_SDK:?WEBOS_SDK must point to arm-webos-linux-gnueabi_sdk-buildroot}"
SYSROOT="$WEBOS_SDK/arm-webos-linux-gnueabi/sysroot"
args=""
for a in "$@"; do
  case "$a" in
    -lstdc++) set -- "$@" "$SYSROOT/usr/lib/libstdc++.a" ;;
    *) set -- "$@" "$a" ;;
  esac
  shift
done
exec "$WEBOS_SDK/bin/arm-webos-linux-gnueabi-g++" --sysroot="$SYSROOT" "$@"

#!/bin/sh
# Builds the F20 firmware binary (build/rockbox.erosq) and lays out the SD
# card as a folder (build/sdcard) with an empty Music directory.
#
# Needs: rustup nightly with rust-src, zig, pkg-config, and a static alsa-lib
# from tools/build-alsa.sh (run that once first).
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
if [ ! -f build/alsa/lib/pkgconfig/alsa.pc ]; then
  echo "build/alsa is missing: run tools/build-alsa.sh first" >&2
  exit 1
fi
if [ "$(cat build/alsa/abi 2>/dev/null)" != "musleabihf" ]; then
  echo "build/alsa was built for a different float ABI: run tools/build-alsa.sh again" >&2
  exit 1
fi
cargo +nightly build --release --target tools/mipsel-f20.json -Z build-std=std,panic_abort -Z json-target-spec "$@"
cp target/mipsel-f20/release/vinyl-f20 build/rockbox.erosq
ls -la build/rockbox.erosq
file build/rockbox.erosq 2>/dev/null || true
"$ROOT/tools/make-sd.sh"

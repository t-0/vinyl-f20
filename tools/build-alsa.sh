#!/bin/sh
# Cross-builds a static alsa-lib for the F20 into build/alsa.
#
# vinyl plays audio through rodio -> cpal -> alsa-lib. Linking our own static
# copy keeps the firmware a single self-contained binary that does not depend
# on the player's libasound.so. The config dir is set to a folder on the SD
# card so alsa-lib finds its alsa.conf (tools/make-sd.sh copies it there).
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
VER=1.2.14
SRC=$ROOT/build/alsa-lib-$VER
PREFIX=$ROOT/build/alsa
TARBALL=$ROOT/build/alsa-lib-$VER.tar.bz2
mkdir -p "$ROOT/build"
[ -f "$TARBALL" ] || curl -fL -o "$TARBALL" "https://www.alsa-project.org/files/pub/lib/alsa-lib-$VER.tar.bz2"
ABI=musleabihf   # must match tools/zig-cc
if [ ! -f "$SRC/src/.libs/libasound.a" ] || [ "$(cat "$PREFIX/abi" 2>/dev/null)" != "$ABI" ]; then
  rm -rf "$SRC"
  tar xjf "$TARBALL" -C "$ROOT/build"
  cd "$SRC"
  CC="$ROOT/tools/zig-cc" AR="$ROOT/tools/zig-ar" RANLIB="zig ranlib" \
  CFLAGS="-O2" \
  ./configure --host=mipsel-linux-musl --prefix="$PREFIX" \
    --enable-static --disable-shared --disable-python --disable-old-symbols \
    --disable-rawmidi --disable-hwdep --disable-seq --disable-ucm --disable-topology \
    --disable-aload --disable-resmgr \
    --with-pcm-plugins="hw,plug,copy,linear,rate,route,empty,softvol,null,file" \
    --with-ctl-plugins="hw,ext,remap" \
    --with-configdir=/mnt/sd_0/.vinyl/alsa --with-plugindir=/mnt/sd_0/.vinyl/alsa/plugins \
    --with-versioned=no > "$ROOT/build/alsa-configure.log" 2>&1
  make -j8 > "$ROOT/build/alsa-make.log" 2>&1
fi
cd "$SRC"
# The library looks for its config under /mnt/sd_0/.vinyl/alsa on the player
# (compiled in above); on this machine the files are installed next to the
# library, and tools/make-sd.sh copies them onto the card.
make install alsaconfigdir="$PREFIX/share/alsa" > "$ROOT/build/alsa-install.log" 2>&1
echo "$ABI" > "$PREFIX/abi"
echo "alsa-lib installed to $PREFIX"
ls -la "$PREFIX/lib"

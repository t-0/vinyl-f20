#!/bin/sh
# Lays the firmware out as an SD card. With no argument it builds the card
# image as a folder, build/sdcard, ready to copy onto a card's root (or give
# the card's mount point, e.g. /Volumes/F20, to write it there directly):
#
#   .rockbox/rockbox.erosq   the player binary (what the Rockbox hosted
#                            bootloader copies to /tmp and runs)
#   .vinyl/config            vinyl's config: music.home = /mnt/sd_0/Music
#   .vinyl/f20.conf          F20-only settings (font, buttons, screen)
#   .vinyl/.asoundrc         ALSA routing: software volume over hw:0,0
#   .vinyl/alsa/             alsa-lib's own config files
#   Music/                   put music here (empty to start with)
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CARD=${1:-$ROOT/build/sdcard}
[ -f "$ROOT/build/rockbox.erosq" ] || { echo "build/rockbox.erosq missing: run tools/build.sh" >&2; exit 1; }
[ -d "$ROOT/build/alsa/share/alsa" ] || { echo "build/alsa missing: run tools/build-alsa.sh" >&2; exit 1; }
mkdir -p "$CARD/.rockbox" "$CARD/.vinyl/alsa" "$CARD/Music"
cp "$ROOT/build/rockbox.erosq" "$CARD/.rockbox/rockbox.erosq"
cp -R "$ROOT/build/alsa/share/alsa/." "$CARD/.vinyl/alsa/"
cp "$ROOT/sd/.vinyl/.asoundrc" "$CARD/.vinyl/.asoundrc"
[ -f "$CARD/.vinyl/f20.conf" ] || cp "$ROOT/sd/.vinyl/f20.conf" "$CARD/.vinyl/f20.conf"
[ -f "$CARD/.vinyl/config" ] || printf 'music.home = /mnt/sd_0/Music\nindex.file = /mnt/sd_0/.vinyl/index.json\n' > "$CARD/.vinyl/config"
echo "SD card layout in $CARD:"
(cd "$CARD" && find . -maxdepth 2 -not -path './.vinyl/alsa/*' | sort)

#!/bin/sh
# Builds FreeType 2.6, the unit's (its /usr/lib/libfreetype.so.6.12.0, which
# stock nlclient sets its text with), into .work/freetype for the desktop
# build, which loads it from there (src/ft.rs): text then comes out as the
# unit's does, to the pixel. Later FreeTypes hint and render differently.
#
#     tools/freetype.sh
#
# Needs a C compiler, make and zlib (its own copy of zlib no longer builds).
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work="$repo/.work/freetype"
url=https://download.savannah.gnu.org/releases/freetype/freetype-old/freetype-2.6.tar.gz
sha256=f5a78da18fa548f6ef325edfbd91d3088c010d896b21863beec5a7ab34188d2c

mkdir -p "$work/src"
tarball="$work/src/freetype-2.6.tar.gz"
[ -f "$tarball" ] || curl -fsSL -o "$tarball" "$url"
if command -v sha256sum > /dev/null; then sum=$(sha256sum "$tarball"); else sum=$(shasum -a 256 "$tarball"); fi
[ "${sum%% *}" = "$sha256" ] || { echo "freetype-2.6.tar.gz: checksum mismatch" >&2; exit 1; }
rm -rf "$work/src/freetype-2.6"
tar xzf "$tarball" -C "$work/src"
cd "$work/src/freetype-2.6"
./configure --prefix="$work" --without-png --without-harfbuzz --without-bzip2 --with-zlib=yes \
    --enable-shared --disable-static > "$work/configure.log" 2>&1
make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)" > "$work/make.log" 2>&1
make install > "$work/install.log" 2>&1
echo "FreeType 2.6 in $work/lib"

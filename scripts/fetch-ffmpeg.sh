#!/usr/bin/env bash
# Put an LGPL ffmpeg where `dx bundle` picks it up as a bundle resource: app/resources/ffmpeg(.exe).
#
#   scripts/fetch-ffmpeg.sh windows | linux | macos
#
# ⚠ The licence is the point of this script. ffmpeg is bundled as a separate executable and must be
# an LGPL build (no --enable-gpl, no --enable-nonfree), or shipping it would put the whole app under
# the GPL. The script REFUSES a build that is not, and refuses one that lacks an encoder the app
# offers. Nothing here is needed on a developer machine: the app finds ffmpeg on PATH when no
# bundled copy exists.
set -euo pipefail

os="${1:?usage: fetch-ffmpeg.sh windows|linux|macos}"
here="$(cd "$(dirname "$0")/.." && pwd)"
dest="$here/app/resources"
work="$(mktemp -d)"
mkdir -p "$dest"

BTBN="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
FFMPEG_VERSION="7.1.1"

case "$os" in
  windows)
    curl -fsSL -o "$work/ffmpeg.zip" "$BTBN/ffmpeg-master-latest-win64-lgpl.zip"
    unzip -j -o "$work/ffmpeg.zip" '*/bin/ffmpeg.exe' -d "$dest"
    bin="$dest/ffmpeg.exe"
    ;;
  linux)
    curl -fsSL -o "$work/ffmpeg.tar.xz" "$BTBN/ffmpeg-master-latest-linux64-lgpl.tar.xz"
    tar -xJf "$work/ffmpeg.tar.xz" -C "$work" --wildcards '*/bin/ffmpeg'
    cp "$work"/*/bin/ffmpeg "$dest/ffmpeg"
    chmod +x "$dest/ffmpeg"
    bin="$dest/ffmpeg"
    ;;
  macos)
    # No ready-made LGPL macOS build is known, so it is built here. The libraries are linked
    # STATICALLY by pointing the linker at a directory that holds only their .a files - Homebrew's
    # own lib directory also has the .dylib, which the linker would prefer.
    brew install lame opus libvorbis libogg nasm pkg-config
    prefix="$(brew --prefix)"
    static="$work/static"
    mkdir -p "$static"
    # LAME 4.0 decodes through mpg123, so its static library needs libmpg123.a beside it: without it
    # configure's test link fails and reports the misleading "libmp3lame >= 3.98.3 not found".
    for lib in libmp3lame libmpg123 libopus libvorbis libvorbisenc libogg; do
      if [ -f "$prefix/lib/$lib.a" ]; then
        cp "$prefix/lib/$lib.a" "$static/"
      elif [ "$lib" != libmpg123 ]; then
        echo "missing $prefix/lib/$lib.a" >&2
        exit 1
      fi
    done
    # Static libraries do not carry their dependencies. pkg-config knows them (libvorbisenc -> libvorbis ->
    # libogg) but names them only for a --static query, so configure is asked to make that query. LAME
    # ships no pkg-config file: libmp3lame.a needs libmpg123.a, which is added by hand.
    extra_libs=""
    [ -f "$static/libmpg123.a" ] && extra_libs="-lmpg123"
    curl -fsSL -o "$work/ffmpeg-src.tar.xz" "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz"
    tar -xJf "$work/ffmpeg-src.tar.xz" -C "$work"
    cd "$work/ffmpeg-$FFMPEG_VERSION"
    ./configure \
      --disable-gpl --disable-nonfree --disable-doc --disable-debug \
      --disable-ffplay --disable-ffprobe \
      --enable-libmp3lame --enable-libvorbis --enable-libopus \
      --extra-cflags="-I$prefix/include" \
      --extra-ldflags="-L$static" \
      --pkg-config-flags="--static" \
      ${extra_libs:+--extra-libs="$extra_libs"} || {
        # The console only says which library; the log says why.
        echo "---- tail of ffbuild/config.log ----" >&2
        tail -80 ffbuild/config.log >&2
        exit 1
      }
    make -j"$(sysctl -n hw.ncpu)"
    cp ffmpeg "$dest/ffmpeg"
    chmod +x "$dest/ffmpeg"
    bin="$dest/ffmpeg"
    ;;
  *)
    echo "unknown os: $os" >&2
    exit 2
    ;;
esac

# ── The licence and capability gate ────────────────────────────────────────────────────────────
info="$("$bin" -hide_banner -version)"
echo "$info" | head -3
# --enable-version3 alone is still LGPL (v3): ffmpeg then reads "LGPL version 3 or later", which is fine
# for a separate executable. Only --enable-gpl / --enable-nonfree change what the app may be shipped as.
if echo "$info" | grep -Eq -- '--enable-(gpl|nonfree)'; then
  echo "REFUSED: this ffmpeg is not an LGPL build:" >&2
  echo "$info" | grep -Eo -- '--enable-(gpl|nonfree)' | sort -u >&2
  exit 1
fi
enc="$("$bin" -hide_banner -encoders)"
for need in libmp3lame aac flac libvorbis libopus wmav2 alac ac3 pcm_s16le pcm_s16be; do
  echo "$enc" | grep -Eq "[[:space:]]$need[[:space:]]" || { echo "REFUSED: ffmpeg has no $need encoder" >&2; exit 1; }
done
echo "ffmpeg OK ($os): $(du -h "$bin" | cut -f1)"
rm -rf "$work"

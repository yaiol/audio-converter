#!/usr/bin/env bash
# Build the command line (yac) in release and put it where `dx bundle` picks it up as a bundle
# resource: app/resources/yac(.exe), beside ffmpeg.
#
#   scripts/build-cli.sh
#
# It is built from the same engine as the window, so there is nothing to keep in step: this only
# compiles it and copies the file.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

cargo build --release -p audio-converter-cli

mkdir -p app/resources
copied=0
for f in target/release/yac target/release/yac.exe; do
  if [ -f "$f" ]; then
    cp "$f" app/resources/
    copied=1
  fi
done
[ "$copied" = 1 ] || { echo "yac was not built" >&2; exit 1; }
ls -l app/resources | grep yac

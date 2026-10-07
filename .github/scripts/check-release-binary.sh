#!/bin/sh
set -eu

binary=${1:?missing binary path}
target=${2:?missing Rust target}

case "$target" in
  aarch64-apple-darwin) arch=arm64 ;;
  x86_64-apple-darwin) arch=x86_64 ;;
  *) echo "Unsupported release target: $target" >&2; exit 1 ;;
esac

test -x "$binary"
test "$(lipo -archs "$binary")" = "$arch"

dependencies=$(otool -L "$binary" | awk 'NR > 1 { print $1 }')
if [ "$dependencies" != /usr/lib/libSystem.B.dylib ]; then
  echo "Unexpected runtime dependencies in $binary: $dependencies" >&2
  exit 1
fi

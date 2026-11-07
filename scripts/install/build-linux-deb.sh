#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAGE="$ROOT/dist/linux/rootfs"
cd "$ROOT"
VERSION="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "dopad-app"))')"
OUTPUT=""

command -v dpkg-deb >/dev/null || { echo "dpkg-deb is required" >&2; exit 1; }
ARCH="$(dpkg --print-architecture)"
OUTPUT="$ROOT/dist/linux/dopad_${VERSION}_${ARCH}.deb"
bash "$ROOT/scripts/install/build-linux.sh"
install -d "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: dopad
Version: $VERSION
Section: editors
Priority: optional
Architecture: $ARCH
Depends: libc6 (>= 2.31), libgcc-s1, libxkbcommon0, libvulkan1, libfontconfig1, libasound2, libwayland-client0, libx11-xcb1
Maintainer: Dopad maintainers
Description: Lightweight native text editor with its command-line client
 Dopad is a lightweight native text editor built with Rust and GPUI.
EOF
dpkg-deb --build --root-owner-group "$STAGE" "$OUTPUT"
test -s "$OUTPUT"
dpkg-deb --contents "$OUTPUT" | grep -F '/usr/bin/dopad' >/dev/null
dpkg-deb --contents "$OUTPUT" | grep -F '/usr/lib/dopad/dopad-gui' >/dev/null
printf 'Built %s\n' "$OUTPUT"

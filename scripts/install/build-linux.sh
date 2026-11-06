#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="$ROOT/target/release"
STAGE="$ROOT/dist/linux/rootfs"

cd "$ROOT"
cargo build --release --locked -p dopad-app -p dopad-cli
install -Dm755 "$TARGET/dopad-app" "$STAGE/usr/lib/dopad/dopad-gui"
install -Dm755 "$TARGET/dopad" "$STAGE/usr/bin/dopad"
install -Dm644 "$ROOT/packaging/linux/dopad.desktop" "$STAGE/usr/share/applications/dopad.desktop"
install -Dm644 "$ROOT/icon.png" "$STAGE/usr/share/icons/hicolor/256x256/apps/dopad.png"

printf 'Staged Dopad GUI and CLI under %s\n' "$STAGE"
"$STAGE/usr/bin/dopad" --version
"$STAGE/usr/bin/dopad" --help >/dev/null

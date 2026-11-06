#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="$ROOT/target/release"
BUNDLE="$ROOT/dist/macos/Dopad.app"
cd "$ROOT"
VERSION="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "dopad-app"))')"

cargo build --release --locked -p dopad-app -p dopad-cli
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources/bin"
install -m 755 "$TARGET/dopad-app" "$BUNDLE/Contents/MacOS/Dopad"
install -m 755 "$TARGET/dopad" "$BUNDLE/Contents/Resources/bin/dopad"
install -m 644 "$ROOT/packaging/macos/Info.plist" "$BUNDLE/Contents/Info.plist"
python3 - "$BUNDLE/Contents/Info.plist" "$VERSION" <<'PY'
import plistlib, sys
path, version = sys.argv[1:]
with open(path, "rb") as source:
    data = plistlib.load(source)
data["CFBundleShortVersionString"] = version
data["CFBundleVersion"] = version
with open(path, "wb") as target:
    plistlib.dump(data, target)
PY

if command -v sips >/dev/null && command -v iconutil >/dev/null; then
  ICONSET="$ROOT/dist/macos/dopad.iconset"
  install -d "$ICONSET"
  for SIZE in 16 32 64 128 256 512; do
    sips -z "$SIZE" "$SIZE" "$ROOT/icon.png" --out "$ICONSET/icon_${SIZE}x${SIZE}.png" >/dev/null
    if [ "$SIZE" -le 256 ]; then
      DOUBLE=$((SIZE * 2))
      sips -z "$DOUBLE" "$DOUBLE" "$ROOT/icon.png" --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" >/dev/null
    fi
  done
  iconutil -c icns "$ICONSET" -o "$BUNDLE/Contents/Resources/dopad.icns"
  python3 - "$BUNDLE/Contents/Info.plist" <<'PY'
import plistlib, sys
path = sys.argv[1]
with open(path, "rb") as source:
    data = plistlib.load(source)
data["CFBundleIconFile"] = "dopad"
with open(path, "wb") as target:
    plistlib.dump(data, target)
PY
fi

"$BUNDLE/Contents/Resources/bin/dopad" --version
"$BUNDLE/Contents/Resources/bin/dopad" --help >/dev/null
printf 'Built unsigned bundle %s\n' "$BUNDLE"
printf 'Sign and notarize the bundle after staging; this script does not alter code signatures.\n'

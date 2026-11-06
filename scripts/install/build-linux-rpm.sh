#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIST="$ROOT/dist/linux"
TOP="$DIST/rpmbuild"
cd "$ROOT"
VERSION="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "dopad-app"))')"
SOURCE="$TOP/SOURCES/dopad-$VERSION-rootfs.tar.gz"

command -v rpmbuild >/dev/null || { echo "rpmbuild is required" >&2; exit 1; }
"$ROOT/scripts/install/build-linux.sh"
install -d "$TOP/BUILD" "$TOP/BUILDROOT" "$TOP/RPMS" "$TOP/SOURCES" "$TOP/SPECS" "$TOP/SRPMS"
tar -C "$DIST/rootfs" -czf "$SOURCE" usr
cp "$ROOT/packaging/linux/dopad.spec" "$TOP/SPECS/dopad.spec"
rpmbuild -bb --define "_topdir $TOP" --define "app_version $VERSION" "$TOP/SPECS/dopad.spec"
find "$TOP/RPMS" -type f -name 'dopad-*.rpm' -print -quit | grep -q .
printf 'Built RPM package under %s/RPMS\n' "$TOP"

#!/usr/bin/env bash
set -euo pipefail

ACTION="install"
APP=""
case "${1:-}" in
  --uninstall) ACTION="uninstall" ;;
  "") ;;
  *) APP="$1" ;;
esac

DEST_DIR="${DOPAD_CLI_INSTALL_DIR:-$HOME/.local/bin}"
DEST="$DEST_DIR/dopad"
MARKER="# Managed by Dopad install-macos-cli.sh"

if [ "$ACTION" = "uninstall" ]; then
  if [ -f "$DEST" ] && grep -Fq "$MARKER" "$DEST"; then
    rm -f -- "$DEST"
    printf 'Removed Dopad CLI launcher: %s\n' "$DEST"
  else
    printf 'No Dopad-managed CLI launcher found at %s\n' "$DEST"
  fi
  exit 0
fi

if [ -z "$APP" ]; then
  APP="$HOME/Applications/Dopad.app"
fi
APP="$(cd "$(dirname "$APP")" && printf '%s/%s' "$PWD" "$(basename "$APP")")"
APP_CLI="$APP/Contents/Resources/bin/dopad"
APP_GUI="$APP/Contents/MacOS/Dopad"
test -x "$APP_CLI" || { printf 'Dopad CLI binary not found: %s\n' "$APP_CLI" >&2; exit 1; }
test -x "$APP_GUI" || { printf 'Dopad Desktop binary not found: %s\n' "$APP_GUI" >&2; exit 1; }

if [ -e "$DEST" ] && ! grep -Fq "$MARKER" "$DEST"; then
  printf 'Refusing to overwrite an existing command: %s\n' "$DEST" >&2
  exit 1
fi

install -d "$DEST_DIR"
CLI_QUOTED="$(printf '%s' "$APP_CLI" | sed "s/'/'\\\\''/g")"
GUI_QUOTED="$(printf '%s' "$APP_GUI" | sed "s/'/'\\\\''/g")"
TEMP="$DEST.tmp.$$"
cat > "$TEMP" <<EOF
#!/bin/sh
$MARKER
export DOPAD_GUI_EXECUTABLE='$GUI_QUOTED'
exec '$CLI_QUOTED' "\$@"
EOF
chmod 755 "$TEMP"
mv -f -- "$TEMP" "$DEST"
printf 'Installed Dopad CLI launcher: %s\n' "$DEST"
case ":$PATH:" in
  *":$DEST_DIR:"*) ;;
  *) printf 'Add %s to PATH to call dopad from new terminals.\n' "$DEST_DIR" ;;
esac

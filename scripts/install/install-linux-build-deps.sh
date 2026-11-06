#!/usr/bin/env bash
set -euo pipefail

packages=(
  cmake
  pkg-config
  libxkbcommon-dev
  libxkbcommon-x11-dev
  libwayland-dev
  libvulkan-dev
  libfontconfig1-dev
  libasound2-dev
  libx11-dev
  libx11-xcb-dev
  libxi-dev
  libxrandr-dev
  libxcursor-dev
  libxinerama-dev
  libxcb1-dev
  libxcb-render0-dev
  libxcb-shape0-dev
  libxcb-xfixes0-dev
  libxcb-randr0-dev
  libxcb-xkb-dev
  libxcb-cursor-dev
  libxcb-ewmh-dev
  libxcb-icccm4-dev
  libegl1-mesa-dev
  libgl1-mesa-dev
)

if [[ "${1:-}" == "--packages" ]]; then
  packages+=(dpkg-dev rpm)
elif [[ -n "${1:-}" ]]; then
  echo "usage: $0 [--packages]" >&2
  exit 2
fi

sudo apt-get update
sudo apt-get install --no-install-recommends --yes "${packages[@]}"

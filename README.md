# Dopad

Dopad is a lightweight native text editor built with Rust, GPUI, and gpui-kit.

## Features

- Fast native text and code editing with tabs and syntax highlighting.
- Markdown and image previews, folder browsing, and a native terminal.
- A separate `dopad` command-line client for opening files and folders.

## Downloads

Download releases from [GitHub Releases](https://github.com/duongonix/dopad/releases).
Release packages target Windows x64 and ARM64, macOS Intel and Apple Silicon, and Linux x64 and ARM64.

macOS bundles are currently unsigned and not notarized, so Gatekeeper may require an explicit user override. Linux archives are built on Ubuntu 24.04 and may require compatible system libraries on other distributions.

## Build from source

Install stable Rust and the native development tools required by GPUI:

- **Windows:** MSVC Rust toolchain, Visual Studio C++ workload, and Windows SDK.
- **macOS:** Xcode Command Line Tools.
- **Linux:** Ubuntu 24.04 or compatible libraries; install the CI build dependencies with `bash scripts/install/install-linux-build-deps.sh`.

Build or run Dopad Desktop and its CLI from the workspace root:

```sh
cargo run --locked -p dopad-app
cargo build --release --locked -p dopad-app -p dopad-cli
```

The release workflow checks that a `vX.Y.Z` tag matches the `dopad-app` version in Cargo metadata. It creates a draft GitHub Release only after all six native builds and package checks pass.

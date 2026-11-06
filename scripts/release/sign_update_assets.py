#!/usr/bin/env python3
"""Create per-target Ed25519-signed metadata for Dopad release packages."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import sys
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


VERSION_PATTERN = re.compile(r"\A(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\Z")
TARGET_PATTERN = re.compile(
    r"\A(?:windows-(?:x86_64|arm64)|macos-(?:x64|arm64)|linux-(?:x86_64|aarch64)-(?:deb|rpm|AppImage))\Z"
)
MAX_PACKAGE_BYTES = 2 * 1024 * 1024 * 1024


def parse_asset(value: str) -> tuple[str, Path]:
    target, separator, raw_path = value.partition("=")
    if not separator or not TARGET_PATTERN.fullmatch(target):
        raise argparse.ArgumentTypeError("asset must be TARGET=PATH with a supported target")
    path = Path(raw_path)
    if not path.is_file():
        raise argparse.ArgumentTypeError(f"package does not exist: {path}")
    return target, path


def write_signed_manifest(
    output: Path,
    version: str,
    target: str,
    package: Path,
    private_key: Ed25519PrivateKey,
) -> None:
    size = package.stat().st_size
    if not 0 < size <= MAX_PACKAGE_BYTES:
        raise ValueError(f"package size is outside the supported range: {package.name}")
    if not package.name.startswith(f"dopad-v{version}-{target}"):
        raise ValueError(f"package name does not bind version and target: {package.name}")

    digest = hashlib.sha256()
    with package.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    manifest = {
        "version": version,
        "target": target,
        "asset_name": package.name,
        "size": size,
        "sha256": digest.hexdigest(),
    }
    message = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode("utf-8")
    signature = base64.b64encode(private_key.sign(message)) + b"\n"

    output.mkdir(parents=True, exist_ok=True)
    (output / f"dopad-update-{target}.json").write_bytes(message)
    (output / f"dopad-update-{target}.sig").write_bytes(signature)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--public-key", default=os.environ.get("DOPAD_UPDATE_PUBLIC_KEY", ""))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--asset", action="append", type=parse_asset, required=True)
    arguments = parser.parse_args()

    if not VERSION_PATTERN.fullmatch(arguments.version):
        parser.error("version must be a stable Cargo semver version without a v prefix")
    if len({target for target, _ in arguments.asset}) != len(arguments.asset):
        parser.error("only one package per target may be signed")
    if len(arguments.public_key) != 64:
        parser.error("DOPAD_UPDATE_PUBLIC_KEY must be 64 hexadecimal characters")
    try:
        expected_public = bytes.fromhex(arguments.public_key)
    except ValueError:
        parser.error("DOPAD_UPDATE_PUBLIC_KEY is malformed")

    private_pem = os.environ.get("DOPAD_UPDATE_PRIVATE_KEY_PEM")
    if not private_pem:
        parser.error("DOPAD_UPDATE_PRIVATE_KEY_PEM secret is required")
    try:
        private_key = serialization.load_pem_private_key(private_pem.encode(), password=None)
    except (ValueError, TypeError) as error:
        parser.error(f"update signing key could not be loaded: {error}")
    if not isinstance(private_key, Ed25519PrivateKey):
        parser.error("update signing key must be Ed25519")
    public_bytes = private_key.public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    if public_bytes != expected_public:
        parser.error("public key does not match DOPAD_UPDATE_PRIVATE_KEY_PEM")

    try:
        for target, package in arguments.asset:
            write_signed_manifest(
                arguments.output, arguments.version, target, package, private_key
            )
    except (OSError, ValueError) as error:
        print(f"sign_update_assets: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

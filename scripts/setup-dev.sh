#!/usr/bin/env bash
# Development setup for Debian/Ubuntu: the pinned Rust toolchain plus the system libraries Tauri
# links against. Idempotent. macOS needs only Xcode Command Line Tools and rustup; Windows needs the
# MSVC build tools and WebView2.
#
# --system-only installs just the system libraries, for environments that provision Rust from
# rust-toolchain.toml themselves.
set -euo pipefail

# The version pinned in rust-toolchain.toml.
RUST_VERSION="$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$(dirname "$0")/../rust-toolchain.toml")"
SYSTEM_ONLY=0
if [ "${1:-}" = "--system-only" ]; then SYSTEM_ONLY=1; fi

if [ "$SYSTEM_ONLY" -eq 0 ] && ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  arch="$(uname -m)-unknown-linux-gnu"
  tmp="$(mktemp -d)"
  curl --proto '=https' --tlsv1.2 -sSf "https://static.rust-lang.org/rustup/dist/${arch}/rustup-init" -o "$tmp/rustup-init"
  curl --proto '=https' --tlsv1.2 -sSf "https://static.rust-lang.org/rustup/dist/${arch}/rustup-init.sha256" -o "$tmp/rustup-init.sha256"
  want="$(cut -d' ' -f1 "$tmp/rustup-init.sha256")"
  got="$(sha256sum "$tmp/rustup-init" | cut -d' ' -f1)"
  [ "$want" = "$got" ] || { echo "rustup-init checksum mismatch" >&2; exit 1; }
  chmod +x "$tmp/rustup-init"
  "$tmp/rustup-init" -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" --component clippy --component rustfmt
  rm -rf "$tmp"
fi

if command -v apt-get >/dev/null 2>&1; then
  SUDO=""
  if [ "$(id -u)" -ne 0 ]; then
    command -v sudo >/dev/null 2>&1 || { echo "setup-dev: installing the system libraries needs root or sudo" >&2; exit 1; }
    SUDO="sudo"
  fi
  export DEBIAN_FRONTEND=noninteractive
  $SUDO apt-get update -qq
  # xvfb and imagemagick are only needed to launch and screenshot the desktop app headlessly.
  $SUDO apt-get install -y -qq --no-install-recommends \
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev \
    pkg-config build-essential xvfb imagemagick
fi

if [ "$SYSTEM_ONLY" -eq 1 ]; then
  echo "ok: system libraries"
else
  echo "ok: $("$HOME/.cargo/bin/cargo" --version 2>/dev/null || cargo --version)"
fi

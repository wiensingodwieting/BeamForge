#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"

# If running on NixOS, use nix-shell to supply Wayland & OpenGL libraries
if [ -f "/etc/NIXOS" ] && command -v nix-shell >/dev/null 2>&1; then
    if [ -f "target/release/beamforge" ]; then
        TMPDIR=/tmp exec nix-shell --run "./target/release/beamforge $*"
    else
        TMPDIR=/tmp exec nix-shell --run "cargo run --release -- $*"
    fi
else
    # Standard Linux distributions (Arch Linux, Ubuntu, Fedora, Debian, etc.)
    if [ -f "target/release/beamforge" ]; then
        exec "./target/release/beamforge" "$@"
    else
        exec cargo run --release -- "$@"
    fi
fi

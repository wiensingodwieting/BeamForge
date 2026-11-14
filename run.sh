#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"

# Reset any broken temporary directory environment variables
export TMPDIR=/tmp
export TEMPDIR=/tmp
export TMP=/tmp

# Add NixOS OpenGL/Vulkan and display driver paths
for p in /run/opengl-driver/lib /run/opengl-driver-32/lib /run/current-system/sw/lib; do
    if [ -d "$p" ]; then
        export LD_LIBRARY_PATH="$p${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi
done

# If running on NixOS, inject cached Wayland, X11 and Slint dependencies
if [ -f "/etc/NIXOS" ]; then
    CACHE_LIB_PATH="$(dirname "$0")/.nix-lib-path"
    if [ ! -s "$CACHE_LIB_PATH" ] && command -v nix-instantiate >/dev/null 2>&1; then
        nix-instantiate --eval --expr 'with import <nixpkgs> {}; lib.makeLibraryPath [ libGL libxkbcommon wayland libx11 libxcursor libxi libxrandr fontconfig freetype ]' 2>/dev/null | tr -d '"' > "$CACHE_LIB_PATH" || true
    fi

    if [ -s "$CACHE_LIB_PATH" ]; then
        NIX_LIBS="$(cat "$CACHE_LIB_PATH")"
        if [ -n "$NIX_LIBS" ]; then
            export LD_LIBRARY_PATH="$NIX_LIBS${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        fi
    fi
fi

BIN="./target/release/beamforge"

# Run compiled release binary directly if present
if [ -x "$BIN" ]; then
    exec "$BIN" "$@"
fi

# Fallback: if binary is missing, compile release directly using cached nix libs
echo "Building BeamForge release binary..."
TMPDIR=/tmp TEMPDIR=/tmp TMP=/tmp cargo build --release
exec "$BIN" "$@"

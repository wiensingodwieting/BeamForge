#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"

echo "=== Сборка BeamForge (Release) ==="
export TMPDIR=/tmp
export TEMPDIR=/tmp
export TMP=/tmp

if [ -f "/etc/NIXOS" ]; then
    nix-shell --run "cargo build --release"
else
    cargo build --release
fi

echo "=== Запуск BeamForge... ==="
./run.sh

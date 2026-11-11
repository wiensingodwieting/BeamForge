#!/usr/bin/env bash
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$HOME/.local/share/applications"

cat > "$HOME/.local/share/applications/beamforge.desktop" <<EOF
[Desktop Entry]
Name=BeamForge
Comment=BeamMP Server Mod & Profile Manager
Exec=$DIR/run.sh
Icon=$DIR/ui/icons/beamforge.svg
Terminal=false
Type=Application
Categories=Game;Utility;
Keywords=beammp;beamng;mods;manager;
EOF

chmod +x "$HOME/.local/share/applications/beamforge.desktop"
echo "Successfully created desktop launcher: $HOME/.local/share/applications/beamforge.desktop"

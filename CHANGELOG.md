# Changelog

All notable changes to **BeamForge** are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.2.0] — 2026-11-12

### 🌟 Highlights
BeamForge 0.2.0 is a major feature update introducing an in-app **Live Server Console**, a graphical **ServerConfig.toml Editor** with automatic mod map detection, and an **HD Widescreen Map Preview** system.

---

### 🚀 New Features

- **📟 Built-in Interactive Server Console Modal**:
  - Live log streaming directly from the `BeamMP-Server` process without needing an external terminal window.
  - Interactive command input (`stdin`) supporting server administrator commands (e.g., `status`, `kick`, `say`).
  - Terminal controls: Status indicator, Clear Console buffer, and hotkey support (`Enter` to submit).

- **⚙️ Visual `ServerConfig.toml` Editor**:
  - Direct in-app configuration of server settings:
    - Server Name & Description
    - Port & AuthKey
    - Max Players & Max Cars per player
    - Private Server toggle (hide from public browser)
  - Safe TOML serialization preventing configuration injection.

- **🗺️ Intelligent Map Selector with Live HD Preview**:
  - Integrated map selector displaying both official BeamNG vanilla maps and scanned mod maps.
  - Automatic archive inspection: extracts internal `/levels/.../info.json` paths and map preview images directly from mod `.zip` archives.
  - Dedicated **290px HD Widescreen Preview Panel**: instantly updates on hover with rich terrain screenshots, map title, category badge (`VANILLA` / `MOD`), and level path.
  - Custom map path manual override for development and testing.

---

### 🎨 UI & UX Polish

- **Sidebar Profile Row**:
  - Restored clean left-aligned profile titles with right-aligned action buttons (Save & Delete).
  - Mathematically centered SVG icons inside hit boxes to prevent edge-clipping.
- **Server Supervisor Card**:
  - Added dedicated 28×28 action buttons for opening the Server Console and Server Settings.
  - Added comfortable spacing (10px) between action buttons and clean margins.
- **Console Input Footer**:
  - Properly bounded input row with 16px lateral margins to eliminate edge bleed.
- **Visual Map Preview**:
  - Streamlined preview card layout with clean widescreen ratio (16:9).

---

### ⚡ Performance & Reliability

- **Instant NixOS Startup**:
  - Optimized `run.sh` with library path caching (`.nix-lib-path`), resolving Wayland/OpenGL dependencies in milliseconds without waiting for `nix-shell` evaluation.
  - Cleaned temporary directory handling (`TMPDIR=/tmp`) preventing stale shell environment lockups.
- **Fault-Tolerant Archive Parsing**:
  - Added regex fallback parsing for non-standard or malformed `info.json` files found in third-party community mods.
- **Security Audit**:
  - Passed Cargo dependency security audit with 0 known vulnerabilities.

---

### 📦 Packaging & CI/CD

- **GitHub Actions Release Pipeline** (`.github/workflows/release.yml`):
  - Automated binary builds and packaging on tag push (`v*`).
  - Generates `beamforge-v{version}-linux-x86_64.tar.gz` with SHA256 checksums and automated release notes.
- **Arch Linux PKGBUILD**:
  - Added PKGBUILD script in `aur/` for Arch Linux users (`makepkg -si`).

---

## [0.1.0] — 2026-11-11

### Initial Release
- Mod indexer and category scanner (Cars, Maps, Wheels, Other).
- Symlink-based mod deployment to `Resources/Client`.
- Profile / preset management (create, switch, delete).
- Search and filtering by status and name.
- Server start/stop process supervisor.
- Setup wizard on first launch.

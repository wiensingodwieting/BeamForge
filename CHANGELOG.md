# Changelog

All notable changes to **BeamForge** are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.3.0] — 2026-11-14

### 🌟 Highlights
BeamForge 0.3.0 is a massive update introducing a high-performance **Asynchronous Streaming Mod Preview System**, an intelligent **Real-Time Mod Conflict Detector**, built-in **Mod Deletion**, one-click **File Manager integration**, pixel-perfect **UI & Layout Alignments**, and an overhauled application icon optimized for modern Wayland desktop environments.

---

### 🚀 New Features & Enhancements

- **⚡ Asynchronous Streaming Preview System (Zero UI Freezes)**:
  - Extracted thumbnail generation into an isolated background worker thread with `std::sync::mpsc` channels.
  - UI streams updates dynamically in real time without stalling startup or interface interaction.
  - In-memory `image_cache` (`HashMap<String, Option<slint::Image>>`) caches loaded images, guaranteeing smooth, stutter-free 60+ FPS scrolling across libraries with 160+ mods.
  - Upgraded preview thumbnail resolution to **52×34px** inside a 44px row height.
  - Previews are cached persistently in `.previews/` and preserved across rebuilds.
  - Heuristic asset detection skips internal material maps (`_d.`, `_n.`, `_spec.`, `_ao.`, `uv_`, skins, liveries) and accurately targets canonical vehicle `default.jpg|png|webp` and map overview screenshots.
  - Clean fallback category badges (`🚗`, `🗺`, `🛞`, `📦`) when no custom image is bundled in the archive.

- **⚠️ Smart Real-Time Mod Conflict Detector**:
  - Automatically scans all active mod zip archives for colliding internal game files (e.g., duplicated `.jbeam` definitions, conflicting vehicle directories, overlapping map assets).
  - Recalculates dynamically in real time on every mod checkbox toggle, `Select all`, and `Deselect all`.
  - Bilateral conflict marking: highlights both active mods that collide with each other with a red `[ ⚠ CONFLICT ]` badge. Clicking the badge reveals exact file paths and colliding mods in the status drawer.
  - Disabling either conflicting mod immediately resolves and clears the warning on both.
  - Global `[⚠ N Conflicts Detected]` counter badge in the top statistics bar.

- **🗑️ In-App Mod Deletion with Confirmation Modal**:
  - Added dedicated delete action button `[🗑]` for each mod item.
  - Features an intuitive modal confirmation dialog showing the mod name.
  - Safely unlinks active symlinks from the server folder, deletes the `.zip` from disk storage, and invalidates cached metadata.

- **📁 Native File Manager Integration ("Show in Folder")**:
  - Added `[📁]` button on mod cards.
  - Leverages standard FreeDesktop D-Bus (`org.freedesktop.FileManager1.ShowItems`) to open the system file manager (Nautilus, Dolphin, etc.) and pre-select/highlight the exact `.zip` archive.
  - Graceful fallback to `xdg-open` parent folder if DBus is unavailable.

- **🎨 Pixel-Perfect UI Centering & Alignment**:
  - Re-architected Slint item layouts: all row controls (checkbox, thumbnail, title, category badge, conflict badge, size, folder button, and trash button) are vertically centered on the 44px row axis.
  - Fixed conflict badge alignment: the `[ ⚠ CONFLICT ]` alert icon now shares the exact vertical center with the text rather than snapping to the top edge.
  - Restored top statistics bar spacing: mod selection stats span the left side while action buttons dock flush to the right edge.
  - Cleaned up top error/status banner with vector SVG alert icon.

- **🖼️ App & Window Icon Redesign**:
  - Optimized `beamforge.svg` by eliminating redundant transparent margins and expanding the squircle container to the full 128×128 canvas.
  - Scaled up the `BF` monogram and forge underline (+28%), matching the visual height and weight of the window title text in niri and other Wayland compositors.

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

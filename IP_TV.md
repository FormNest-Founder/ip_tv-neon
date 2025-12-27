# PROJECT MATRIX: ip_tv-neon (Night City Hub)

## 🌌 ARCHITECTURE & STACK
- **Language:** Rust (Edition 2024 optimized).
- **Frontend:** TUI via `ratatui` + `crossterm`.
- **Backend:** `tokio` (Async runtime), `reqwest` (Network), `quick-xml` (EPG Parsing).
- **Persistence:** `bincode` (Binary cache in `/tmp/neon_iptv_rs`), `serde` (JSON config).
- **System Integration:** `ksni` (D-Bus Tray), `mpv` (External player).

## 🛠 CORE PATHS
- **Binary:** `/usr/bin/ip_tv` (Installed globally).
- **Source:** `~/Git/ip_tv-neon/src/main.rs`.
- **Config:** `~/.config/neon-iptv/config.json`.
- **Cache:** `/tmp/neon_iptv_rs/data.bin` (M3U + EPG index).

## 🚀 KEY FEATURES & LOGIC
### 1. Intelligent EPG Mapping (Turbo Search)
- **Pre-normalization:** All channel names are stripped of junk (HD, FHD, +, etc.) and brand prefixes (BCU, YOSSO, VF).
- **O(1) Lookup:** A `HashMap<String, String>` maps normalized names to XMLTV IDs during the UPDATE cycle.
- **Search:** Instant filtering in `ChanList`. Searching works against both raw and normalized names.

### 2. MPV Integration (Haswell Optimized)
- **Engine:** `vo=gpu-next` + `hwdec=vaapi`.
- **HDR-to-SDR:** Method `bt.2390` with peak compute disabled for Iris Pro 5200.
- **Auto-Scale:** Heavy 4K/HDR streams automatically fallback to `bilinear` scaling to prevent lags.
- **Shaders:** Integrated FSR (Upscale) and CAS (Sharpening) via `~/.config/mpv/input.conf`.

### 3. Radio Metadata
- **Dynamic:** Fetches live track info (Artist - Song) from Radio Record API on entering the Radio screen.
- **Tray:** Persists metadata in system tray via `ksni`.

## 🎨 UI DESIGN (NIGHT CITY NEON)
- **Colors:** 
  - `Cyan`: Brand Prefixes (BCU, BOX, etc.)
  - `Green`: HD/FHD quality tags.
  - `Red`: 4K tags.
  - `Magenta`: Current EPG program.
  - `Yellow`: Radio stations.
  - `Hot Pink`: Active radio tracks.
- **Detail Screen:** Displays full program description and coming schedule.

## ✅ COMPLETED TASKS
- [x] Radical performance refactor (zero-lag lists).
- [x] Description support in Detail view.
- [x] Prefix-aware EPG mapping (BCU/VF fix).
- [x] Metadata integration for Radio Record.
- [x] Cleanup of legacy Python/Fish scripts.

## 📋 NEXT STEPS / IDEAS
- [ ] Adaptive EPG refresh (auto-update every 24h).
- [ ] Category filtering via hotkeys.
- [ ] Integration of custom user streams via local M3U.

---
*This file is the primary context for any Gemini session dealing with IPTV/Media Hub.*

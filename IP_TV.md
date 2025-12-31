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

### 2. MPV Integration (Haswell Ultra Optimized)
- **Engine:** `vo=gpu-next` + `hwdec=auto-copy` (Core combination for performance/quality).
- **Format:** `fbo-format=rgba16hf` + `opengl-pbo=yes` (Reduces memory bandwidth bottlenecks).
- **Scaling:** `ewa_lanczos` in **Linear Light** (`linear-upscaling=yes`).
- **Levels:** Forced `video-output-levels=full` to fix washed-out blacks on iMac display.
- **Dithering:** `fruit` (8-tap) for smooth gradients on 8-bit panel.
- **Shaders:** RGB-fixed CAS (`CAS.glsl`) for sharpening + `deband` (2 iterations).
- **Auto-Scale:** Heavy 4K streams fallback to `bilinear` via profiles.

### 3. Radio Metadata
- **Dynamic:** Fetches live track info (Artist - Song) from Radio Record API on entering the Radio screen.
- **Tray:** Persists metadata in system tray via `ksni`.

### 4. Local Library Scanner (New in v0.7.0)
- **Source:** Scans `~/Downloads` for `.m3u` and `.m3u8` files.
- **Playback:** Direct file playback via MPV (`file://...`).
- **UI:** Dedicated "LOCAL" menu button.

## 🎨 UI DESIGN (NIGHT CITY NEON)
- **Colors:** 
  - `Cyan`: Brand Prefixes (BCU, BOX, etc.)
  - `Green`: HD/FHD quality tags.
  - `Red`: 4K tags.
  - `Magenta`: Current EPG program.
  - `Yellow`: Radio stations.
  - `Hot Pink`: Active radio tracks.
  - `Dark Green`: Local files.
- **Detail Screen:** Displays full program description and coming schedule.

## ✅ COMPLETED TASKS
- [x] Radical performance refactor (zero-lag lists).
- [x] Description support in Detail view.
- [x] Prefix-aware EPG mapping (BCU/VF fix).
- [x] Metadata integration for Radio Record.
- [x] Cleanup of legacy Python/Fish scripts.
- [x] **v0.7.0:** Removed TorrServer, added Local Scanner.
- [x] **v0.7.1:** TimeShift (Archives) support via `tvg-rec`, EPG Sorting, Live Fallback.

## 📋 NEXT STEPS / IDEAS
- [ ] Deep Search (Global program search).
- [ ] Adaptive EPG refresh (auto-update every 24h).
- [ ] Category filtering via hotkeys.

---
*This file is the primary context for any Gemini session dealing with IPTV/Media Hub.*

### Update 2025-12-30: Waybar Radio Volume Control
- Добавлено управление громкостью MPV напрямую через Waybar.
- Механика: Прокрутка колеса мыши ( / ) над модулем .
- Команда: .

### Update 2025-12-30: Waybar Radio Volume Control
- Добавлено управление громкостью MPV напрямую через Waybar.
- Механика: Прокрутка колеса мыши (on-scroll-up / on-scroll-down) над модулем custom/radio.
- Команда: playerctl -p mpv volume 0.05+/-

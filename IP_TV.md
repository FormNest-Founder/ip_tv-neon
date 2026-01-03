# 📘 TECHNICAL REFERENCE MANUAL: IP_TV Neon (Night City Hub)

## 📌 OVERVIEW
**IP_TV Neon** is a high-performance, terminal-based IPTV and Radio player written in Rust. It features a Cyberpunk-themed TUI (Text User Interface), advanced EPG management, Timeshift support, and tight system integration (MPV, Waybar).

## 📁 FILE STRUCTURE & RESPONSIBILITIES

### 1. `src/main.rs` (Entry Point & Event Loop)
- **Role:** Orchestrates the application lifecycle, handles user input, and manages screen transitions.
- **Key Components:**
  - `main()`: Initializes raw terminal mode, loads config, creates `App` state, and runs the event loop.
  - **Event Loop:** Polls for keyboard events every 100ms.
  - **Screen Logic:**
    - `Screen::MainMenu`: Navigation between app sections.
    - `Screen::ChanList`: Channel filtering (search), selection, and playing.
    - `Screen::Detail`: EPG navigation, description viewing, and **Timeshift playback**.
    - `Screen::Radio*`: Radio station selection and playback.
- **Key Features:**
  - **Search:** Case-insensitive, real-time filtering in `ChanList`.
  - **Timeshift:** Logic in `Screen::Detail` -> `Enter` checks if a program is in the past and appends `?utc={start}&lutc={now}` to the URL.

### 2. `src/app.rs` (Application State)
- **Role:** Holds the runtime state of the application.
- **Struct:** `App`
  - `config`: User settings (URLs, colors).
  - `data`: Loaded M3U/EPG data (`AppData`).
  - `screen`: Current active screen (`enum Screen`).
  - `states`: `ListState` for various TUI lists (main, channels, radio, etc.).
  - `clean_regex`: Pre-compiled Regex (`r"^(BCU|BOX|VF|YOSSO|VIP)\s+"`) for cleaning channel names.
- **Functions:**
  - `new(config)`: Loads data from binary cache (`data.bin`), initializes states and regex.
  - `run_mpv(...)`: Spawns MPV subprocess in detached mode. Sets window title and geometry.
  - `stop_all()`: Kills existing MPV instances via `pkill`.

### 3. `src/ui.rs` (Rendering Engine)
- **Role:** Draws the TUI widgets based on the current `App` state.
- **Key Function:** `ui(f: &mut Frame, app: &mut App)`
- **Screens Rendered:**
  - `MainMenu`: List of main options.
  - `ChanList`:
    - **Visual Timeline:** Calculates `pct` (progress) for current program and renders a bar `[███░░░]`.
    - **Name Cleaning:** Uses `app.clean_regex` to strip prefixes.
  - `Detail` (EPG):
    - **Dynamic Layout:** Calculates required height for the Description block based on text length.
    - **Info Block:** Shows full description with wrapping.
  - `RadioList`: Displays stations with "Now Playing" track info.

### 4. `src/epg.rs` (Data Ingestion)
- **Role:** Fetches, parses, and processes M3U playlists and XMLTV EPGs.
- **Key Function:** `update_data(config)` (Async)
  - **Radio API:** Fetches stations from Radio Record API + Zaycev.fm hardcoded list.
  - **Radio Tracks:** Calls `fetch_radio_now()` to get current songs.
  - **M3U Parsing:** regex-based parsing of `#EXTINF`. Normalizes names.
  - **EPG Parsing:** Streaming XML parser (`quick-xml`).
    - Extracts `start`, `stop`, `title`, `desc`.
    - **Filter:** Keeps programs from `now - 24h` to future (for Timeshift).
  - **Cache:** Serializes `AppData` to `~/.cache/neon-iptv/data.bin` using `bincode`.

### 5. `src/models.rs` (Data Structures)
- **Role:** Defines the data types used throughout the app.
- **Structs:**
  - `Config`: JSON-serializable settings.
  - `Channel`: `name`, `group`, `url`, `tvg_id`, `logo`, `catchup_days`.
  - `RadioStation`: `title`, `stream`, `track` (current song).
  - `EpgProgram`: `start`, `stop`, `title`, `desc`.
  - `AppData`: The "database" holding all vectors and hashmaps.
  - `Screen`: Enum for navigation state.

### 6. `src/utils.rs` (Helpers)
- **Role:** Utility functions.
- **Functions:**
  - `get_cache_dir()`: Returns absolute path to `~/.cache/neon-iptv/`.
  - `main_log(msg)`: Writes debug info to `/tmp/neon_iptv.log`.
  - `normalize(s)`: Strings -> alphanumeric lowercase (for fuzzy matching).
  - `parse_xml_time(s)`: Parses XMLTV timestamps (various formats).

---

## 🎬 MPV PLAYER INTEGRATION

**Goal:** Provide a seamless, high-performance playback experience optimized for Haswell architecture (Intel Iris Pro 5200) on Linux.

### 1. Command Construction (`src/app.rs`)
The `run_mpv` function builds the shell command dynamically:

```rust
let mut c = Command::new("mpv");
c.arg(format!("--title=NEON: {}", display_title))
 .arg(format!("--force-media-title={}", display_title))
 .arg("--volume=15")
 .arg("--ontop")
 .arg("--hwdec=vaapi-copy")
 .arg("--vo=gpu-next")
 .arg("--no-ytdl");
```

### 2. Performance Arguments (Critical for Haswell)
- **`--vo=gpu-next`**: The modern video output driver in MPV. Essential for correct color handling and shader support.
- **`--hwdec=vaapi-copy`**:
  - Uses Intel QuickSync (via VA-API) to decode video.
  - `copy` mode allows shaders and filters to run on the decoded frames (vs `vaapi` direct render).
  - **Target:** Reduces CPU usage from ~60% to ~5-10% for 1080p streams.
- **`--no-ytdl`**: Disables internal youtube-dl hook to speed up stream opening (IPTV streams are direct links).

### 3. 4K / HDR Handling (Heavy Streams)
If the title contains "4K" or "HDR", extra flags are applied to prevent stuttering on the older GPU:
- **`--hdr-compute-peak=no`**: Disables expensive HDR peak detection.
- **`--tone-mapping=bt.2390`**: Fast, efficient tone mapping algorithm.
- **`--scale=bilinear`**: Uses simplest scaling algorithm to save GPU cycles.

### 4. Radio Mode
If `radio: true` is passed (for Radio channels):
- **`--no-video`**: Disables video decoding/rendering entirely. Saves massive resources.

### 5. Window Management
- **Fullscreen:** Controlled by `config.video_fullscreen` (Default: `true` -> `--fs`).
- **Geometry:** If not fullscreen, uses `--geometry=1280x720` (from config).
- **Detached Process:**
  - Uses `libc::setsid()` to detach MPV from the terminal.
  - Standard I/O streams (`stdout`, `stderr`) are piped to `/dev/null` to prevent noise.

---

## 🎧 WAYBAR INTEGRATION

### Capsule Architecture (Top-Bar)
The Waybar module `custom/radio` provides a dynamic, scrolling "capsule" that displays the currently playing media (Radio or TV) with control features.

### 1. 📜 Marquee Scroller (`radio_scroller.py`)
- **Path:** `~/.local/bin/radio_scroller.py`
- **Logic:**
  - Polls `playerctl metadata` every 0.2s.
  - **Output:** JSON for Waybar (`{"text": "...", "class": "toxic-radio", "tooltip": "..."}`).
  - **Animation:** If text length > 30 chars, it rotates the string to create a marquee effect.
  - **Fallback:** Reads `/tmp/radio_station` if MPV is not providing metadata.
  - **Idle State:** Returns empty JSON (hides module) if MPV is not running.

### 2. 🔊 Volume Control (`volume_radio.sh`)
- **Path:** `~/.local/bin/volume_radio.sh`
- **Trigger:** Mouse Scroll Up/Down on the capsule.
- **Logic:**
  - `up`: `playerctl -p mpv volume 0.05+`
  - `down`: `playerctl -p mpv volume 0.05-`
  - **Feedback:** Sends a notification (`notify-send`) with the current volume percentage.

### 3. 🖱 Interaction Menu (`waybar_radio_click.sh`)
- **Path:** `~/.local/bin/waybar_radio_click.sh`
- **Trigger:** Left Click on the capsule.
- **Logic:**
  - Checks if `mpv` is running.
  - **Running:** Opens a `fuzzel` menu with:
    - ⏹️ STOP (Kills MPV)
    - 📻 CHANGE STATION (Restarts `neon` / `ip_tv`)
  - **Not Running:** Launches `fish -c neon` (Main Menu).

---

## 📁 SYSTEM PATHS & DEPLOYMENT
- **Binary (Executable):** `/usr/local/bin/ip_tv`
  - *Deployed via:* `cargo build --release && sudo mv target/release/ip_tv-neon /usr/local/bin/ip_tv`
- **Source Code Repository:** `~/Gemini/ip_tv-neon/` (or `~/Git/ip_tv-neon/`)

### Key Functional Flows
1.  **Startup:** `main.rs` loads `Config`, tries to load `data.bin`. If missing, triggers `update_data`.
2.  **Navigation:** `main.rs` listens for keys. `ChanList` allows filtering via typing.
3.  **Playback:** User hits `Enter`. `main.rs` constructs URL (adding Timeshift params if needed) and calls `app.run_mpv`.
4.  **UI Update:** `ui.rs` redraws every frame/event. `ChanList` updates progress bars in real-time.

---
*Created by Gemini SRE Agent for Night City System.*

# NEON IPTV — The Only Terminal IPTV Player with a Built-in AI Brain

**Ask your TV "what comedies are on tonight?" and get results you can play with one keypress.**

NEON IPTV is a 2700-line Rust TUI that combines an IPTV/Radio player with a live LLM assistant. The AI sees your full EPG (thousands of programs across hundreds of channels), understands your viewing history, and finds exactly what you want — in natural language, in under 2 seconds.

No Electron. No browser. No 500MB RAM for a channel list. Just a 4.3MB static binary, 15MB RSS, and mpv doing what mpv does best.

![Main Menu](screenshots/main.png)

## What the AI Actually Does

This isn't a chatbot bolted onto a player. The LLM is wired into the EPG search engine:

1. **You ask** — "horror movies tonight", "что идёт на спортивных каналах", "podborka multfilmov"
2. **AI analyzes** — sees live EPG across all channels + your viewing history, generates targeted search keywords
3. **Search engine fires** — TF-IDF scoring across every program title and description, live results ranked first
4. **You play** — select any result, hit Enter. Live stream, archive, or timeshift — one keypress

![AI Chat — natural language search across all channels](screenshots/ai_chat.png)

Supports two LLM providers — **DeepSeek V3** (~$0.001/query) and **Google Gemini 2.5 Flash** (free tier available). Switch between them in Settings with one keypress. Without an API key, everything else works normally.

## Why Rust + mpv

- **4.3MB binary, 15MB RSS** — cold start under 100ms. Binary cache (bincode) means zero re-parsing on launch
- **mpv as the player** — not a built-in video widget. Your system mpv with your configs, your shaders (`CAS`, `FSR`), your `vaapi`/`vulkan` hwdec. The app manages mpv as an async child process and gets out of the way
- **Fully async** — tokio runtime: HTTP fetches, mpv lifecycle, radio track info, AI chat — nothing blocks the UI
- **Terminal-native** — runs over SSH, in tmux, on a headless server driving a TV

## Screenshots

### Channel Browser — live EPG with progress bars
![Channels](screenshots/channels.png)

### Detail Screen — full schedule with archive playback
![Detail](screenshots/detail.png)

### Radio — 100+ stations with live track info
![Radio](screenshots/radio.png)

## Features

### IPTV
- **Channel browser** — categories with contextual icons (country flags, content type icons) and channel counts
- **EPG integration** — current program with live progress bar (gradient green → yellow → red)
- **TimeShift / Archive** — playback past programs via catchup URLs (`tvg-rec` days auto-detection)
- **Channel markers** — `★` favorites, `⏪` archive-capable channels
- **Detail screen** — full EPG schedule with times, descriptions, current program highlight
- **Theme-consistent UI** — accent color applied to category browser, favorites, history, settings, local playlists

### Radio
- **Radio Record** stations with genre categories and contextual icons
- **Now Playing** — async track info fetch (artist — song), non-blocking
- Background playback — TUI stays interactive while radio plays

### AI Chat (optional)
- Smart TV assistant powered by **DeepSeek V3** or **Google Gemini 2.5 Flash** (switchable in Settings)
- **Context-aware**: feeds current EPG across channels + your viewing history to the LLM
- Auto-extracts keywords from AI response and searches EPG in real-time
- Split-screen UI: search results (top) + chat (bottom)
- Cost: ~$0.001 per query (DeepSeek V3) / free tier available (Gemini)

### Video Playback
- **mpv** as an external player — your config, your shaders, your hwdec
- Async process management via `tokio::process` with 12h timeout
- Fullscreen or windowed mode (configurable geometry)
- Suspended mode: TUI hides during video, auto-restores after mpv exits
- Radio: mpv runs in background (`--no-video`), TUI stays interactive

### Performance
- **4.3MB** fat LTO binary (Clang + LLD, target-cpu optimized, codegen-units=1, strip=symbols)
- **~15MB RSS** — no Electron, no WebView, no garbage collector
- **Instant startup** — versioned binary cache (bincode), no re-parsing on launch
- **Non-blocking I/O** — async everything: HTTP fetches, mpv management, track info, AI chat, data updates
- **Zero-copy XML parsing** — EPG parsed with `quick-xml` enum-based state machine, no String allocations in hot path

### Other
- **Favorites** — persistent set, toggle with `f`
- **History** — last 200 watched streams (deduplicated)
- **Local playlists** — scans configurable directory (or default `~/`, `~/Downloads/`, `~/Videos/`) for `.m3u`/`.m3u8` files
- **7 color themes** — Cyan, Magenta, Neon Green, Orange, Purple, Yellow, Red
- **Contextual icons** — ~40 country flags for IPTV categories, ~25 genre icons for radio
- **Panic hook** — terminal is always restored on crash
- **Diagnostics** — `ip_tv diag` shows all paths, URLs, cache state

## Requirements

- **Linux** (tested on Arch/CachyOS)
- **mpv** — media player (`pacman -S mpv` / `apt install mpv`)
- **Rust toolchain** — for building from source
- **niri** (optional) — Wayland compositor for suspended video mode. Without niri, video plays without TUI hiding
- **DeepSeek API key** (optional) — for AI Chat with DeepSeek provider
- **Gemini API key** (optional) — for AI Chat with Google Gemini provider

## Installation

### Build from source

```bash
git clone https://github.com/nicorp/ip_tv-neon.git
cd ip_tv-neon
cargo build --release
```

### Install binary

```bash
# Copy to a directory in your $PATH
cp target/release/ip_tv-neon ~/.local/bin/ip_tv

# Or install via cargo
cargo install --path .
```

### Verify

```bash
ip_tv diag
```

## Usage

```bash
ip_tv              # Launch TUI
ip_tv update        # Update data (playlist + EPG + radio) without TUI
ip_tv diag          # Show diagnostics (paths, URLs, cache info)
ip_tv --debug       # Launch with debug logging to /tmp/neon_iptv.log
```

## First Run

1. **Launch:** `ip_tv`
2. **Set playlist:** Settings → Playlist URL → paste your M3U/M3U8 URL (or local path)
3. **Set EPG:** Settings → EPG URL → paste your EPG source (default is pre-filled: `epg.one`)
4. **Update data:** Main Menu → Update — downloads playlist, EPG, and radio stations
5. **Watch:** Navigate to IPTV → pick a category → pick a channel → Enter

## Configuration

### File Locations

| Path | Description |
|------|-------------|
| `~/.config/neon-iptv/config.json` | Main config (URLs, theme, favorites, history) |
| `~/.cache/neon-iptv/data.bin` | Binary data cache (channels, EPG, radio) |
| `/tmp/neon_iptv.log` | Debug log (only with `--debug`) |
| `/tmp/neon_mpv.log` | MPV log (only with `--debug`) |

Config and cache directories follow the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/). On most Linux systems:
- Config: `~/.config/neon-iptv/`
- Cache: `~/.cache/neon-iptv/`

### config.json

Created automatically on first run. You can edit it manually or via the in-app Settings screen.

```json
{
  "playlist_url": "https://example.com/playlist.m3u",
  "epg_url": "http://epg.one/epg.xml.gz",
  "theme_color": [0, 255, 255],
  "favorites": [],
  "history": [],
  "video_fullscreen": true,
  "video_geometry": "1280x720",
  "local_dir": "",
  "llm_provider": ""
}
```

| Field | Type | Description |
|-------|------|-------------|
| `playlist_url` | string | URL to M3U/M3U8 playlist, or local file path |
| `epg_url` | string | URL to XMLTV EPG (supports `.xml.gz` gzip). Default: `http://epg.one/epg.xml.gz` |
| `theme_color` | [R, G, B] | UI accent color. See theme presets below |
| `favorites` | string[] | Stream URLs marked as favorites |
| `history` | string[] | Last 200 watched stream URLs (auto-managed) |
| `video_fullscreen` | bool | Launch mpv in fullscreen mode |
| `video_geometry` | string | Window size when fullscreen is off (e.g. `"1920x1080"`) |
| `local_dir` | string | Custom directory for local `.m3u`/`.m3u8` files. Empty = scan default dirs |
| `llm_provider` | string | AI provider: `"deepseek"` (default) or `"gemini"` |

### Settings Screen

| # | Setting | Type | Description |
|---|---------|------|-------------|
| 1 | Playlist URL | edit | M3U/M3U8 playlist source |
| 2 | EPG URL | edit | XMLTV EPG source |
| 3 | Fullscreen | toggle | mpv fullscreen on/off |
| 4 | Window Geometry | edit | mpv window size (e.g. `1280x720`) |
| 5 | Theme | toggle | Cycle through 7 color presets |
| 6 | AI Provider | toggle | Switch between DeepSeek and Gemini |
| 7 | Local Playlists Dir | edit | Directory to scan for local `.m3u` files |
| 8 | Clear History | action | Delete all history entries |
| 9 | Clear Favorites | action | Delete all favorites |

### Theme Presets

| Name | RGB |
|------|-----|
| Cyan | `[0, 255, 255]` |
| Magenta | `[255, 0, 255]` |
| Neon Green | `[0, 255, 128]` |
| Orange | `[255, 128, 0]` |
| Purple | `[128, 0, 255]` |
| Yellow | `[255, 255, 0]` |
| Red | `[255, 0, 0]` |

### AI Chat Setup (optional)

The AI Chat feature supports two providers. Configure one or both:

#### DeepSeek (default)

1. Get an API key at https://platform.deepseek.com/
2. Set the environment variable:

```bash
# Add to your shell profile (~/.bashrc, ~/.config/fish/config.fish, etc.)
export DEEPSEEK_API_KEY="sk-your-key-here"
```

#### Google Gemini

1. Get an API key at https://aistudio.google.com/apikey
2. Add to system environment:

```bash
# /etc/environment (system-wide) or shell profile
GEMINI_API_KEY="your-api-key-here"
```

Switch between providers in Settings → AI Provider (press Enter to toggle).

Without any API key, all other features work normally — AI Chat will show an error message when accessed.

## Playlist Format

Standard M3U/M3U8 with extended attributes:

```m3u
#EXTM3U
#EXTINF:-1 tvg-id="channel.id" tvg-name="Channel Name" group-title="Category" tvg-rec="7",Channel Name
http://stream.example.com/live/stream.m3u8

#EXTGRP:Another Category
#EXTINF:-1,Simple Channel
http://another.stream/live
```

| Attribute | Description |
|-----------|-------------|
| `tvg-id` | EPG channel ID (for matching EPG data) |
| `tvg-name` | Display name override |
| `group-title` | Category name |
| `tvg-rec` | Archive days available (enables TimeShift) |

Playlists can be loaded from:
- **HTTP/HTTPS URL** — set in config or Settings
- **Local file** — set path in Playlist URL, or use the Local Playlists screen (scans configured directory or defaults)

## EPG Format

Standard [XMLTV](http://wiki.xmltv.org/index.php/XMLTVFormat) format, optionally gzip-compressed (`.xml.gz`):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<tv>
  <channel id="channel.id">
    <display-name>Channel Name</display-name>
  </channel>
  <programme start="20260314120000 +0300" stop="20260314130000 +0300" channel="channel.id">
    <title>Program Title</title>
    <desc>Program description</desc>
  </programme>
</tv>
```

EPG channels are matched to playlist channels by `tvg-id` first, then by normalized channel name.

## Keyboard Controls

### Global

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate lists |
| `Enter` | Select / Play |
| `Esc` | Go back / Stop playback |
| `Ctrl+C` | Quit |

### Channel List

| Key | Action |
|-----|--------|
| `f` | Toggle favorite |
| `d` | Open channel detail (EPG schedule) |
| Type letters | Real-time search filter |
| `Backspace` | Clear search character |

### Detail Screen (EPG)

| Key | Action |
|-----|--------|
| `Enter` | Play selected program (live or archive) |
| `l` | Play live stream |
| `f` | Toggle favorite |
| `Esc` | Back to channel list |

### AI Chat

| Key | Action |
|-----|--------|
| `Tab` | Toggle focus between results and chat input |
| Type letters | Chat input |
| `Enter` | Send message / Play selected result |
| `d` | Open channel detail for selected result |
| `Esc` | Back to main menu |

### Settings

| Key | Action |
|-----|--------|
| `Enter` | Edit setting / Toggle / Execute action |
| Type | Edit value |
| `Enter` | Save edited value |
| `Esc` | Cancel edit |

## Architecture

```
src/
├── main.rs     533 lines  Event loop, async tasks, key handling, suspended mode
├── ui.rs       826 lines  Screen rendering (ratatui widgets, themes, icons, layout)
├── app.rs      396 lines  App state, mpv launch, filters, navigation
├── ai.rs       456 lines  LLM chat (DeepSeek + Gemini), EPG context, keyword extraction
├── epg.rs      300 lines  M3U/EPG/radio parsing, XML parser, cache, local scanning
├── models.rs   159 lines  Data structures (Config, Channel, Screen, AppData)
├── utils.rs     28 lines  Normalize, XML time parser, logging
└── consts.rs    27 lines  Constants, XDG paths, API endpoints
                ──────
                ~2725 lines total
```

### Data Flow

```
Playlist URL (M3U)  ──→  epg.rs (parse)  ──→  AppData.channels
EPG URL (XMLTV)     ──→  epg.rs (parse)  ──→  AppData.epg
Radio API           ──→  epg.rs (fetch)  ──→  AppData.radio
                                              ↓
                                         data.bin (bincode cache)
                                              ↓
                                         app.rs (App state)
                                              ↓
                                         ui.rs (render) ──→ Terminal
                                              ↓
                                         mpv (external player)
```

### AI Pipeline

```
User query ──→ LLM (DeepSeek or Gemini)
                    ↓
              system prompt (EPG summary + history)
                    ↓
              AI response + extracted keywords
                    ↓
              TF-IDF search across all EPG programs
                    ↓
              ranked results (live > future > archive)
                    ↓
              select + Enter → mpv playback
```

## Tech Stack

| Component | Crate/Tool | Purpose |
|-----------|------------|---------|
| Async runtime | `tokio` | Non-blocking I/O, process management |
| TUI framework | `ratatui` + `crossterm` | Terminal UI rendering |
| HTTP client | `reqwest` | Playlist/EPG/API fetching |
| XML parser | `quick-xml` | EPG XMLTV parsing |
| Compression | `flate2` | Gzip EPG decompression |
| Serialization | `bincode` + `serde` | Binary cache, JSON config |
| Regex | `regex` | M3U attribute parsing |
| Time | `chrono` | EPG time parsing, UTC operations |
| Media player | `mpv` (external) | Stream playback |
| AI (option 1) | DeepSeek V3 API | TV assistant |
| AI (option 2) | Gemini 2.5 Flash API | TV assistant |
| Build | Clang + LLD | Fat LTO, codegen-units=1, strip=symbols |

## Troubleshooting

### No channels after update
- Check your playlist URL in Settings — it must be a valid M3U/M3U8
- Run `ip_tv diag` to verify paths and URLs
- Check if the URL is accessible: `curl -I "your-playlist-url"`

### No EPG data
- Verify EPG URL in Settings
- EPG matching uses `tvg-id` from playlist — ensure your playlist has `tvg-id` attributes
- Run update again — EPG download can be slow for large files

### mpv doesn't start
- Ensure mpv is installed: `mpv --version`
- Check stream URL manually: `mpv "stream-url"`
- Use `--debug` flag and check `/tmp/neon_mpv.log`

### Cache issues
- Delete cache to force full reload: `rm ~/.cache/neon-iptv/data.bin`
- Cache auto-resets when the app version changes

### AI Chat shows error
- **DeepSeek:** Set `DEEPSEEK_API_KEY` environment variable. Verify: `curl -H "Authorization: Bearer $DEEPSEEK_API_KEY" https://api.deepseek.com/v1/models`
- **Gemini:** Set `GEMINI_API_KEY` environment variable. Verify: `curl "https://generativelanguage.googleapis.com/v1beta/models?key=$GEMINI_API_KEY"`
- Switch provider in Settings → AI Provider if one isn't working

## About

Written by [Claude](https://claude.ai) (Anthropic) under the architectural direction of a human SRE. Every design decision, feature scope, and code review — human. Every line of code — AI. This is what human-AI collaboration looks like when the human knows what they want and the AI knows how to build it.

## License

MIT

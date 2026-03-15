# NEON IPTV — Terminal IPTV & Radio Player

A fast, async TUI player for IPTV streams, internet radio, and EPG — built with Rust.

Supports M3U/M3U8 playlists, XMLTV electronic program guide, timeshift archive playback, and an optional AI-powered TV assistant via DeepSeek API.

## Features

### IPTV
- **Channel browser** — categories with channel counts, real-time search
- **EPG integration** — current program with live progress bar (gradient green → yellow → red)
- **TimeShift / Archive** — playback past programs via catchup URLs (`tvg-rec` days auto-detection)
- **Channel markers** — `★` favorites, `⏪` archive-capable channels
- **Detail screen** — full EPG schedule with times, descriptions, current program highlight

### Radio
- **Radio Record** stations with genre categories
- **Now Playing** — async track info fetch (artist — song), non-blocking
- Background playback — TUI stays interactive while radio plays

### AI Chat (optional)
- Smart TV assistant powered by DeepSeek API
- Context-aware: sees current EPG across channels + your viewing history
- Auto-searches EPG by keywords extracted from AI response
- Split-screen UI: search results (top) + chat (bottom)
- Requires `DEEPSEEK_API_KEY` environment variable

### Video Player
- External **mpv** player (launched via `tokio::process`)
- Fullscreen or windowed mode (configurable geometry)
- Video suspended mode: hides TUI during playback, auto-restores after mpv exits
- Radio: mpv runs in background (`--no-video`), TUI stays visible

### Other
- **Favorites** — persistent set, toggle with `f`
- **History** — last 200 watched streams (deduplicated)
- **Local playlists** — scans `~/`, `~/Downloads/`, `~/Videos/` for `.m3u`/`.m3u8` files
- **7 color themes** — Cyan, Magenta, Neon Green, Orange, Purple, Yellow, Red
- **Binary cache** — fast startup via versioned bincode cache
- **Panic hook** — terminal is always restored on crash
- **Diagnostics** — `ip_tv diag` shows all paths, URLs, cache state

## Screenshots

*Coming soon*

## Requirements

- **Linux** (tested on Arch/CachyOS)
- **mpv** — media player (`pacman -S mpv` / `apt install mpv`)
- **Rust toolchain** — for building from source
- **niri** (optional) — Wayland compositor for suspended video mode. Without niri, video plays without TUI hiding
- **DeepSeek API key** (optional) — for AI Chat feature

## Installation

### Build from source

```bash
git clone https://github.com/user/ip_tv-neon.git
cd ip_tv-neon
cargo build --release
```

### Install binary

```bash
# Copy to a directory in your $PATH
cp target/release/ip_tv ~/.local/bin/

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
  "video_geometry": "1280x720"
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

The AI Chat feature uses [DeepSeek API](https://platform.deepseek.com/) (`deepseek-chat` model).

1. Get an API key at https://platform.deepseek.com/
2. Set the environment variable:

```bash
# Add to your shell profile (~/.bashrc, ~/.config/fish/config.fish, etc.)
export DEEPSEEK_API_KEY="sk-your-key-here"
```

Without the key, all other features work normally — AI Chat will show an error message when accessed.

## First Run

1. **Launch:** `ip_tv`
2. **Set playlist:** Settings → Playlist URL → paste your M3U/M3U8 URL (or local path)
3. **Set EPG:** Settings → EPG URL → paste your EPG source (default is pre-filled: `epg.one`)
4. **Update data:** Main Menu → Update — downloads playlist, EPG, and radio stations
5. **Watch:** Navigate to IPTV → pick a category → pick a channel → Enter

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
- **Local file** — set path in Playlist URL, or use the Local Playlists screen (scans `~/`, `~/Downloads/`, `~/Videos/`)

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
| `Esc` | Back to channel list |

### AI Chat

| Key | Action |
|-----|--------|
| `Tab` | Toggle focus between results and chat input |
| Type letters | Chat input |
| `Enter` | Send message / Play selected result |
| `d` | Open channel detail for selected result |

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
├── main.rs     505 lines  Event loop, async tasks, key handling, suspended mode
├── ui.rs       702 lines  Screen rendering (ratatui widgets, themes, layout)
├── app.rs      370 lines  App state, mpv launch, filters, navigation
├── ai.rs       336 lines  DeepSeek chat, EPG context, keyword extraction
├── epg.rs      282 lines  M3U/EPG/radio parsing, XML parser, cache
├── models.rs   151 lines  Data structures (Config, Channel, Screen, AppData)
├── utils.rs     28 lines  Normalize, XML time parser, logging
└── consts.rs    27 lines  Constants, XDG paths, API endpoints
                ──────
                2401 lines total
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
| Media player | `mpv` (external) | Stream playback |
| AI | DeepSeek API | TV assistant (optional) |

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
- Set `DEEPSEEK_API_KEY` environment variable
- Verify key works: `curl -H "Authorization: Bearer $DEEPSEEK_API_KEY" https://api.deepseek.com/v1/models`

## License

MIT

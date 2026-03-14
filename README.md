# NEON HUB — IPTV/Radio TUI Player

Terminal media hub for IPTV and Radio. Rust + ratatui + MPV.

## Features

### IPTV
- Channel categories with instant counts
- Real-time search across channels
- EPG: progress bar with gradient (green/yellow/orange), current program title
- Channel markers: `★` favorite, `⏪` archive
- Detail screen: full EPG schedule with times, descriptions, current program highlight

### TimeShift (Archive)
- Auto-detection of `tvg-rec` days from playlist
- Past programs playback via catchup URL (`?utc=START&lutc=STOP`)
- Archive programs marked with `⏪` in detail screen

### AI Chat (DeepSeek)
- Smart TV assistant — recommends movies/shows from current broadcast
- Sees what's playing NOW across all channels (EPG context)
- Personalized suggestions based on viewing history
- Split-screen: results (top) + chat (bottom)
- Searches EPG by specific titles extracted from AI response

### Radio
- Radio Record stations with genres
- Now playing track info (artist — song)
- Non-blocking async track fetch

### Suspended Mode (Video)
- Video launch hides TUI window (niri IPC → workspace 4)
- TUI auto-restores after MPV closes
- Radio: background process, TUI stays visible

### Other
- Favorites and history (dedup, limit 200)
- Settings: playlist URL, EPG URL, fullscreen, geometry, theme (7 presets)
- Local playlists (.m3u/.m3u8)
- Binary cache with versioning (auto-reset on update)
- Panic hook: terminal restores on crash

## Controls

| Key | Action |
|-----|--------|
| `↑/↓` | Navigate |
| `Enter` | Select / play |
| `Esc` | Back / stop MPV |
| `f` | Toggle favorite |
| `l` | Play live (Detail screen) |
| `d` | Channel details (AI results) |
| `Tab` | Toggle focus (AI Chat) |
| Letters | Search / chat input |
| `Ctrl+C` | Quit |

## Install

```bash
# Build (requires Rust toolchain)
cargo build --release
cp target/release/ip_tv ~/.local/bin/
```

## Usage

```bash
ip_tv            # Launch TUI
ip_tv --debug    # With debug log (/tmp/neon_iptv.log)
ip_tv update     # Update cache (playlist + EPG + radio)
ip_tv diag       # Diagnostics (paths, URLs, cache state)
```

## Configuration

| File | Purpose |
|------|---------|
| `~/.config/neon-iptv/config.json` | Settings (URLs, theme, favorites, history) |
| `~/.cache/neon-iptv/data.bin` | Binary data cache (bincode, versioned) |

### Environment

```bash
# Required for AI Chat feature
export DEEPSEEK_API_KEY="your-api-key"
```

### First Run

1. Launch `ip_tv`
2. Go to Settings → Playlist URL → enter your M3U/M3U8 playlist URL
3. Go to Settings → EPG URL → enter EPG source (default: epg.one)
4. Select Update from main menu

## Stack

- **Rust** + tokio (async runtime)
- **ratatui** 0.29 + crossterm (TUI)
- **reqwest** (HTTP client)
- **quick-xml** (EPG XML parser)
- **bincode** (binary cache)
- **MPV** (external player via `tokio::process`)
- **DeepSeek API** (AI chat, optional)

## Architecture

```
main.rs    — Event loop, async tasks, suspended mode, key handling
app.rs     — App state, MPV launch, filters, AI play
ai.rs      — DeepSeek chat, EPG context builder, keyword extraction, EPG search
ui.rs      — Screen rendering (ratatui widgets)
epg.rs     — Playlist/EPG/radio parsing, XML parser
models.rs  — Data structures (Config, Channel, Screen, AppData)
utils.rs   — normalize, parse_xml_time, logging
consts.rs  — Constants, XDG paths, cache version
```

## License

MIT

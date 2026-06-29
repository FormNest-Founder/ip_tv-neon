# NEON-IPTV — Terminal IPTV/TV player with a built-in AI brain

**Ask your TV "what comedies are on tonight?" and get a list you can play with one keypress.**

NEON-IPTV is a [ratatui](https://ratatui.rs/) terminal application that turns your shell into a full IPTV/TV
hub: a channel browser with live EPG, an electronic programme grid, a radio player with VU meters, and a built-in
LLM assistant that reads your whole EPG and finds exactly what you want — in natural language. Playback runs in
[mpv](https://mpv.io/), driven over a JSON IPC socket; the TUI never blocks while you watch or listen.

No Electron, no browser, no embedded video widget. A single Rust binary, your own mpv, and a binary cache so
startup is instant.

![Main Menu](screenshots/main.png)

---

## What it is

NEON-IPTV bundles four things behind one neon TUI:

- **IPTV / TV** — M3U/M3U8 playlists grouped into categories, each channel showing its current programme with a
  live progress bar; a detail screen with the full schedule and archive/timeshift playback.
- **EPG** — XMLTV (optionally gzip-compressed) ingested into a per-channel programme list, matched to channels by
  `tvg-id` first, then by normalized display name.
- **AI assistant** — a chat panel wired into the EPG search engine. It recommends what to watch, answers
  "what's on now?", and emits keywords the player uses to search every programme title and description across
  every channel.
- **Radio** — Radio Record stations by genre, with live "now playing" track info, an animated AIMP-style VU
  meter, a marquee, and a volume slider — all while the channel list above stays navigable.

---

## Features

### IPTV / TV
- **Channel browser** — categories with contextual icons (~40 country flags + content-type icons) and channel counts.
- **Live EPG inline** — current programme with a gradient progress bar (green → yellow → red) next to each channel.
- **Detail / EPG screen** — full schedule with start/end times, per-programme description, current-programme highlight.
- **TimeShift / archive** — replay past programmes through catchup URLs (`?utc=…&lutc=…`), auto-enabled from the
  `tvg-rec` attribute. Archive-capable channels are marked `⏪`.
- **Favorites & history** — `★` favorites toggled with `f`; the last 200 watched streams, deduplicated.
- **Real-time search** — type to filter the channel list; falls back to a global search if the current category
  has no match.
- **7 color themes** — Cyan, Magenta, Neon Green, Orange, Purple, Yellow, Red.

### EPG ingest
- XMLTV parser built on `quick-xml` (enum state machine, no per-event String allocations in the hot path).
- Transparent gzip (`.xml.gz`) decompression via `flate2`.
- Hardened against decompression bombs and OOM with hard byte/entry caps (see [Resource bounds](#resource-bounds)).
- Untrusted title/description text is terminal-sanitized at the parse boundary, so no crafted EPG entry can inject
  ANSI/control sequences into your terminal.

### AI assistant
- Chat panel that feeds the current EPG (what's on now across channels) plus your viewing history to an LLM.
- The model replies in natural language and appends a `KEYWORDS: …` line; the player runs a **genre-aware,
  word-boundary** EPG search and ranks results **live → score → start time**.
- Split-screen UI: search results on top, chat at the bottom; jump between them with `Tab`.
- **Multi-backend model catalog** — DeepSeek and Gemini over their HTTP APIs, plus five keyless backends served
  through the local `agy` CLI (Gemini 3.5 Flash / 3.1 Pro, Claude Sonnet 4.6, Claude Opus 4.6, GPT-OSS 120B).
  See [The AI assistant](#the-ai-assistant).
- **Customizable prompt** — drop a `~/.config/neon-iptv/ai_prompt.md` to retune behavior (including a personal
  taste profile) without rebuilding. A fixed anti-injection role preamble is always prepended and cannot be
  overridden by the file or by any host environment.

### Radio
- Radio Record stations grouped by genre with contextual icons.
- Async "now playing" (artist – track) fetched without blocking the UI.
- Compact neon player panel: animated VU meters (20 FPS), marquee of `Station │ Artist │ Track`, live bitrate,
  and a volume slider — driven over mpv IPC.
- Background playback: the station list stays interactive while audio plays; `↑↓ Enter` switch stations live.

### Playback
- **mpv as the player** — your install, your config, your shaders and hwdec. The app spawns mpv as an async child
  and gets out of the way.
- TV streams open in an mpv window with HLS cache tuning; hardware decoding and GPU APIs are deferred to your `mpv.conf` to ensure safe cross-platform defaults; radio runs windowless
  (`--no-video`) under an IPC socket.
- **Protocol whitelist** — only `http://` / `https://` media URLs ever reach mpv; `file://`, `edl://` and other
  pseudo-protocols a crafted playlist might inject are blocked before playback starts.
- **Panic hook** — the terminal (raw mode, alternate screen) is always restored, even on a crash.

### Performance
- Single Rust binary (~8 MB release build), low RSS, no GC, no WebView.
- Versioned **bincode** cache (`data.bin`) — channels/EPG/radio are deserialized on launch instead of re-parsed.
- Fully async on `tokio`: HTTP fetches, mpv lifecycle, radio metadata, AI chat and data updates never block the UI.

---

## Install & run

### Requirements
- **Linux** (developed on Arch/CachyOS).
- **mpv** — `pacman -S mpv` / `apt install mpv`.
- **Rust toolchain** — to build from source.
- *(optional)* a **DeepSeek** or **Gemini** API key for those backends, or the **`agy`** CLI for the keyless AGY backends.

### Build

```bash
git clone https://github.com/FormNest-Founder/ip_tv-neon.git
cd ip_tv-neon
cargo build --release
```

### Install the binary

```bash
cp target/release/ip_tv-neon ~/.local/bin/
# (optional) shorten the command:
#   ln -s ~/.local/bin/ip_tv-neon ~/.local/bin/ip_tv
```

### Commands

```bash
ip_tv-neon           # launch the TUI
ip_tv-neon update    # refresh playlist + EPG + radio without opening the TUI
ip_tv-neon diag      # print config/cache paths, URLs and cache state
ip_tv-neon --debug   # launch with extra mpv/app logging
```

### First run

1. Launch `ip_tv-neon`.
2. **Settings → Playlist URL** → paste your M3U/M3U8 URL (or a local file path).
3. **Settings → EPG URL** → paste your XMLTV source. A default is pre-filled: `https://epg.one/epg.xml.gz`.
4. **Main Menu → 🔄 Update** → downloads playlist, EPG and radio stations into the cache.
5. **IPTV → category → channel → Enter** to open the detail screen, then `Enter`/`l` to play.

The config file is created the first time a setting is saved. The cache is created by the first update.

---

## Configuration

### File locations (XDG)

| Path | What |
|------|------|
| `~/.config/neon-iptv/config.json` | Main config (URLs, theme, AI provider, favorites, history). Written `0600`, atomic replace. |
| `~/.config/neon-iptv/ai_prompt.md` | Optional AI system-prompt override. **You create this** — it is never written by the app. |
| `~/.cache/neon-iptv/data.bin` | Binary cache (channels, EPG, radio), prefixed with a schema version. |
| `~/.cache/neon-iptv/neon_iptv.log` | App log — error/security events (e.g. blocked URL scheme, cache invalidation, EPG cap hit). |
| `$TMPDIR/neon_mpv.log` | mpv's own log — only when launched with `--debug`. |
| `$TMPDIR/neon_mpv_stderr.log` | Radio mpv stderr (always captured for radio). |

Paths follow the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/).

### config.json

```json
{
  "playlist_url": "https://example.com/playlist.m3u",
  "epg_url": "https://epg.one/epg.xml.gz",
  "theme_color": [0, 255, 255],
  "favorites": [],
  "history": [],
  "channel_names": {},
  "video_fullscreen": true,
  "video_geometry": "1280x720",
  "local_dir": "",
  "llm_provider": "deepseek"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `playlist_url` | string | M3U/M3U8 URL, or a local file path. |
| `epg_url` | string | XMLTV EPG URL (supports `.xml.gz`). Default: `https://epg.one/epg.xml.gz`. |
| `theme_color` | `[R,G,B]` | UI accent color (one of the theme presets below). |
| `favorites` | string[] | Stream URLs marked favorite. |
| `history` | string[] | Last 200 watched stream URLs (auto-managed). |
| `channel_names` | object | URL → name cache so favorites/history render without a loaded playlist (auto-managed). |
| `video_fullscreen` | bool | Launch the mpv window fullscreen. |
| `video_geometry` | string | mpv window size when fullscreen is off (e.g. `1920x1080`). |
| `local_dir` | string | Directory to scan for local `.m3u`/`.m3u8`. Empty = scan `~/`, `~/Downloads/`, `~/Videos/`. |
| `llm_provider` | string | Selected AI model — a catalog **id token** (see [The AI assistant](#the-ai-assistant)). Empty / unknown → `deepseek`. |

### Settings screen

Open with **Main Menu → ⚙ Settings**. `↑↓` move, `Enter` edits or toggles, `Esc` goes back.

| # | idx | Setting | Action |
|---|-----|---------|--------|
| 1 | 0 | Playlist URL | edit |
| 2 | 1 | EPG URL | edit |
| 3 | 2 | Fullscreen | toggle |
| 4 | 3 | Window Geometry | edit |
| 5 | 4 | Theme | toggle (cycles 7 presets) |
| 6 | 5 | **AI Provider** | toggle (cycles the model catalog) |
| 7 | 6 | Local Playlists Dir | edit |
| 8 | 7 | Clear History | action |
| 9 | 8 | Clear Favorites | action |

### Theme presets

| Name | RGB | | Name | RGB |
|------|-----|-|------|-----|
| Cyan | `[0,255,255]` | | Purple | `[128,0,255]` |
| Magenta | `[255,0,255]` | | Yellow | `[255,255,0]` |
| Neon Green | `[0,255,128]` | | Red | `[255,0,0]` |
| Orange | `[255,128,0]` | | | |

### The cache & `CACHE_SCHEMA_VERSION`

`data.bin` is a `bincode`-serialized `CacheContainer { version: u32, data: AppData }`. On load the app reads the
first 4 little-endian bytes; if they don't equal the current `CACHE_SCHEMA_VERSION` (**currently `2`**), the cache
is deleted and rebuilt on the next update — no panic, no stale-struct deserialize.

**Bump policy:** increment `CACHE_SCHEMA_VERSION` (in `src/consts.rs`) **whenever the bincode layout of
`CacheContainer`, `AppData`, `Channel`, `RadioStation` or `EpgProgram` changes** (add/remove/reorder a field, change
a type). This is independent of the app/package version, so adding a field that only the cache stores never forces an
app version bump. The JSON `config.json` schema is separate — e.g. `llm_provider` is stored as a `String`, so adding
models never touches the cache schema.

### Resource bounds

EPG and playlist URLs are user-editable and point at third-party servers, so every byte read is bounded before
allocation (defends against decompression bombs / OOM):

| Limit | Value |
|-------|-------|
| EPG download (compressed/raw) | 128 MiB |
| EPG after gzip decompression | 768 MiB |
| EPG `<programme>` entries | 4,000,000 |
| EPG distinct channel names | 200,000 |
| Playlist download | 64 MiB |
| Channels parsed per playlist | 500,000 |
| `agy` subprocess wall-clock | 90 s (killed + reaped on timeout) |

Hitting a cap is logged loudly to `neon_iptv.log` and truncates rather than crashing. The decompressed cap is sized
with headroom over the default source (epg.one ≈ 417 MB decompressed as of 2026-06), so legitimate data is never cut.

---

## The AI assistant

Open it from **Main Menu → 🤖 AI Chat**.

### How the search works
You type a request; the model replies (in Russian, concise) and — when you're looking for content — appends a final
line `KEYWORDS: a, b, c`. The player then searches the **entire** EPG (every channel, the full schedule including
archive) for those keywords and shows ranked results you can play with one keypress. The model is told to emit
**specific, recognizable film/series titles** rather than broad genre words, because the search is title-weighted.

### Genre-aware, word-boundary search
Keyword matching is **whole-word and Unicode-aware**, not raw substring. A short keyword like `оно` matches the
standalone title «Оно» but never the incidental substring inside `регионов`. A result qualifies only on a *strong*
title hit (a distinctive keyword — a phrase or an 8+ char word — or a keyword that *leads* the title) or on **≥2**
distinct keyword hits anywhere; titles score above descriptions. A small stop-word list drops generic terms
("фильм", "сериал", "movie", "best", …). The net effect: results stay on-genre instead of being polluted by random
substring collisions.

### Model catalog
Cycle through these in **Settings → AI Provider (idx 5)** by pressing `Enter`. The selected entry's **id token** is
saved to `config.json` → `llm_provider`.

| id token (`llm_provider`) | Label | Backend | Auth |
|---|---|---|---|
| `deepseek` *(default)* | DeepSeek | DeepSeek HTTP API (`deepseek-chat`) | `DEEPSEEK_API_KEY` |
| `gemini` | Gemini (API) | Google Generative Language API (`gemini-2.5-flash`) | `GEMINI_API_KEY` |
| `agy:gemini-3.5-flash` | AGY · Gemini 3.5 Flash | local `agy` CLI | keyless (agy login) |
| `agy:gemini-3.1-pro` | AGY · Gemini 3.1 Pro | local `agy` CLI | keyless (agy login) |
| `agy:claude-sonnet-4-6` | AGY · Claude Sonnet 4.6 | local `agy` CLI | keyless (agy login) |
| `agy:claude-opus-4-6` | AGY · Claude Opus 4.6 | local `agy` CLI | keyless (agy login) |
| `agy:gpt-oss-120b` | AGY · GPT-OSS 120B | local `agy` CLI | keyless (agy login) |

Unknown/empty tokens fall back to `deepseek`; the legacy bare `gemini` value still resolves to the Gemini API row.

### API-key backends (DeepSeek / Gemini)

```bash
# DeepSeek — https://platform.deepseek.com/
export DEEPSEEK_API_KEY="sk-..."

# Gemini  — https://aistudio.google.com/apikey
export GEMINI_API_KEY="..."
```

Put these in your shell profile or `/etc/environment` (the app reads them from the process environment).

### AGY backends (keyless)
The `AGY · …` entries shell out to the local [`agy`](https://github.com/FormNest-Founder) (Antigravity) CLI in
one-shot print mode — **no API key needed**, but:

- `agy` must be installed at `~/.local/bin/agy` (preferred) or anywhere on `PATH`, and
- you must be **logged in** (`agy` → open the OAuth URL → paste the code).

If `agy` isn't found, the AI Provider line shows `(agy not found)` and AGY queries return a loud Russian error
instead of a blank reply. Each AGY call is hard-capped at 90 s.

### Custom prompt & taste profile
The system prompt is `ROLE_PREAMBLE` (fixed, anti-injection, TV-assistant role) + a **body**. The body comes from
`~/.config/neon-iptv/ai_prompt.md` if it exists, otherwise a built-in default. Create the file to retune behavior —
for example, add a personal taste section so recommendations match you:

```markdown
You are NEON AI, a personal TV/film expert.

## USER TASTE
- Loves: Villeneuve, A24, slow sci-fi, Scandinavian noir, stand-up.
- Avoids: laugh-track sitcoms, reality TV, dubbed-over-original anime.
- Prefers original audio with subtitles.

When recommending, weight toward the "Loves" list and end with:
KEYWORDS: title1, title2, title3
```

The preamble is always prepended and **cannot** be overridden by this file — the assistant keeps its TV role on
every backend, and untrusted channel/EPG text is treated as data, never as instructions.

---

## Keybindings

Read straight from the input handlers in `main.rs`.

### Global
| Key | Action |
|-----|--------|
| `↑` / `↓` | Move selection |
| `Enter` | Select / open / play |
| `Esc` | Back one screen (or stop playback) |
| `Ctrl+C` | Quit (from menus and lists) |

### Channel list
| Key | Action |
|-----|--------|
| `Enter` | Open the channel's detail/EPG screen |
| `f` | Toggle favorite for the highlighted channel |
| *type letters* | Live search filter |
| `Backspace` | Delete one search character |
| `Esc` | Clear search, back to categories |

> Note: in the channel list `f` always toggles favorite, so it can't be typed into the search box; every other
> letter filters.

### Detail / EPG screen
| Key | Action |
|-----|--------|
| `↑` / `↓` | Move through programmes |
| `Enter` | Play the selected programme (live → direct, past+archive → catchup URL) |
| `l` | Play the live stream |
| `f` | Toggle favorite |
| `Esc` | Back |

### Radio (while a station plays)
| Key | Action |
|-----|--------|
| `Space` | Pause / resume |
| `+` / `=` / `-` | Volume ±5 |
| `m` | Mute / unmute |
| `↑` / `↓` | Switch station (list stays live) |
| `Enter` | Play the highlighted station |
| `Esc` | Stop and close the player |

### AI Chat
| Key | Action |
|-----|--------|
| *type letters* | Chat input |
| `Enter` | Send the message (when input is focused) |
| `Tab` | Toggle focus between chat input and results |
| `↑` / `↓` | Move through results (when results are focused) |
| `Enter` | Play the selected result (when results are focused) |
| `d` | Open the result's channel detail (when results are focused) |
| `Esc` | Back to the main menu |

### TV playback
While an mpv video window is open the TUI shows a "NOW PLAYING" overlay; press `Esc` to stop and return.

---

## Playlist format

Standard M3U/M3U8 with extended attributes:

```m3u
#EXTM3U
#EXTINF:-1 tvg-id="channel.id" tvg-name="Channel Name" group-title="Category" tvg-rec="7",Channel Name
http://stream.example.com/live/stream.m3u8

#EXTGRP:Another Category
#EXTINF:-1,Simple Channel
http://another.stream/live
```

| Attribute | Meaning |
|-----------|---------|
| `tvg-id` | EPG channel id (primary EPG match key) |
| `tvg-name` | Display-name fallback used as the EPG id if `tvg-id` is absent |
| `group-title` / `#EXTGRP:` | Category |
| `tvg-rec` | Archive days available → enables TimeShift |

Channels without a URL are dropped. Names, group titles and EXTGRP values are terminal-sanitized.

## EPG format

Standard [XMLTV](http://wiki.xmltv.org/index.php/XMLTVFormat), optionally gzip-compressed:

```xml
<tv>
  <channel id="channel.id"><display-name>Channel Name</display-name></channel>
  <programme start="20260314120000 +0300" stop="20260314130000 +0300" channel="channel.id">
    <title>Program Title</title>
    <desc>Program description</desc>
  </programme>
</tv>
```

EPG is matched to playlist channels by `tvg-id`, then by normalized `display-name`. Programmes that ended more than
a day ago are dropped at parse time; the detail screen extends the window back by `tvg-rec` days for archive replay.

---

## Architecture (Decoupled & SOLID)

```
src/
├── main.rs     721 lines   Event loop, decoupled screen-specific input controllers
├── ui.rs      1220 lines   ratatui rendering — data-driven icons, screens, themes
├── app.rs      489 lines   Domain App state, config, filters, search logic
├── player.rs   273 lines   PlayerController: OS processes, mpv execution, IPC lifecycle
├── ai.rs       849 lines   Model catalog, DeepSeek/Gemini/AGY backends, AI search
├── epg.rs      512 lines   M3U/EPG/radio ingest, XMLTV parser, bincode cache
├── mpv_ipc.rs  231 lines   mpv JSON IPC over Unix sockets
├── models.rs   232 lines   Config, Channel, EpgProgram, AppData, Screen, ViewStates
├── consts.rs    64 lines   Versions, API endpoints, static UI data
├── utils.rs     39 lines   normalize, XML time parse, terminal sanitizer, logging
└── lib.rs        5 lines   Test surface (ai, consts, epg, models, utils)
```

### Data flow
```
Playlist (M3U) ┐
EPG (XMLTV)    ┼─→ epg.rs parse ─→ AppData ─→ data.bin (bincode cache) ─→ App ─→ ui.rs ─→ terminal
Radio API      ┘                                                          └─→ mpv (external player)
```

### AI pipeline
```
user query ─→ LLM (DeepSeek | Gemini | AGY)  ── system: role preamble + prompt + EPG/now + history
                          ↓
              reply text + "KEYWORDS: …"
                          ↓
              genre-aware word-boundary EPG search (title-weighted)
                          ↓
              ranked results (live → score → start) ─→ Enter ─→ mpv
```

## Tech stack

| Area | Crate / tool |
|------|--------------|
| Async runtime | `tokio` |
| TUI | `ratatui` + `crossterm` |
| HTTP | `reqwest` |
| XML | `quick-xml` |
| Gzip | `flate2` |
| Serialization | `bincode` (cache) + `serde`/`serde_json` (config) |
| Regex | `regex` |
| Time | `chrono` |
| Player | `mpv` (external, JSON IPC) |
| AI | DeepSeek API, Gemini API, `agy` CLI |

## Tests

```bash
cargo test
```

Covers keyword extraction, the model catalog (unique ids, cycle wrap, legacy/unknown resolution, agy resolver),
EPG-search relevance (the genre-agnostic word-boundary fix and its collision classes), EPG id matching, config
round-trip, and the cache version-prefix invariant.

## License

MIT — see [LICENSE](LICENSE).

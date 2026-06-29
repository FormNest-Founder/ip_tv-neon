# NEON-IPTV — FAQ

Practical answers, grounded in how the code actually behaves. For the full reference see [README.md](README.md).

---

### How do I add a playlist or EPG URL?

Two ways:

1. **In-app:** Settings → *Playlist URL* / *EPG URL* → `Enter` to edit → type/paste → `Enter` to save. Then
   Main Menu → 🔄 **Update**.
2. **By file:** edit `~/.config/neon-iptv/config.json` (`playlist_url`, `epg_url`) while the app is closed, then run
   `ip_tv-neon update`.

`playlist_url` accepts an HTTP/HTTPS URL **or** a local file path. `epg_url` accepts XMLTV, plain or `.xml.gz`
(gzip is detected from the `.gz` extension or the gzip magic bytes). The EPG default is
`https://epg.one/epg.xml.gz`. Nothing appears until you run **Update** — that's what downloads and caches everything.

---

### Nothing shows up after I set the URLs. Why?

The TUI reads from the cache (`data.bin`); the cache is only filled by **Update**. Run Main Menu → 🔄 Update (or
`ip_tv-neon update`), watch the status line for `Updated: N ch, M radio, K EPG`, then browse. Run `ip_tv-neon diag`
to confirm the paths and that the playlist/EPG URLs are what you expect.

---

### How do I pick an AI model?

Settings → **AI Provider** (the 6th row, settings index 5) → press `Enter` to cycle the catalog. The choice is
saved as a short id token in `config.json` → `llm_provider`. The catalog:

| Pick | Needs |
|------|-------|
| **DeepSeek** *(default)* | `DEEPSEEK_API_KEY` in the environment |
| **Gemini (API)** | `GEMINI_API_KEY` in the environment |
| **AGY · Gemini 3.5 Flash / 3.1 Pro** | `agy` CLI installed + logged in (keyless) |
| **AGY · Claude Sonnet 4.6 / Opus 4.6** | `agy` CLI installed + logged in (keyless) |
| **AGY · GPT-OSS 120B** | `agy` CLI installed + logged in (keyless) |

If you pick an AGY model and `agy` isn't found, the line reads `… (agy not found)` and queries return a clear
error instead of silence.

---

### How do I set the DeepSeek / Gemini keys?

```bash
export DEEPSEEK_API_KEY="sk-..."     # https://platform.deepseek.com/
export GEMINI_API_KEY="..."          # https://aistudio.google.com/apikey
```

Add them to your shell profile or `/etc/environment`. The app reads them from its own process environment, so start
NEON-IPTV from a shell where the variable is exported (or log in again after editing `/etc/environment`). Verify:

```bash
curl -s -H "Authorization: Bearer $DEEPSEEK_API_KEY" https://api.deepseek.com/v1/models | head
curl -s "https://generativelanguage.googleapis.com/v1beta/models?key=$GEMINI_API_KEY" | head
```

---

### How do the AGY (keyless) backends work?

The `AGY · …` models run the local `agy` (Antigravity) CLI in one-shot print mode — no API key. Requirements:

1. `agy` at `~/.local/bin/agy` (preferred path) or anywhere on `PATH`.
2. Logged in: run `agy`, open the OAuth URL it prints, paste the code back.

The app flattens the prompt + recent history into one argument list (no shell is invoked), runs `agy` with a 90 s
timeout, and on failure returns a loud Russian message — it never hangs or returns a blank line. Check `agy` health
independently with `agy -p "ping" --model gemini-3.5-flash`.

---

### Why does the assistant answer in Russian?

The fixed role preamble instructs it to reply in Russian to match the primary user, and to act only as a TV/EPG
helper. You can change tone/length in `ai_prompt.md`, but the preamble (role + anti-injection) is always prepended
and can't be overridden — so even if a channel name or your global `agy`/`GEMINI.md` config tries to give the model
a different persona, it stays the TV assistant.

---

### How do I customize the prompt and add a taste profile?

Create `~/.config/neon-iptv/ai_prompt.md` (the app **never creates it for you** — it only reads it). Its contents
replace the *body* of the system prompt. A taste section is the highest-value edit:

```markdown
You are NEON AI, a personal film/TV expert.

## USER TASTE
- Loves: hard sci-fi, A24, Nordic noir, documentary, stand-up.
- Avoids: laugh-track sitcoms, reality TV.
- Original audio + subtitles over dubs.

Recommend toward "Loves", then output:
KEYWORDS: title1, title2, title3
```

No rebuild needed — the file is read on each query. Delete it to fall back to the built-in default.

---

### Why are search results now genre-accurate?

Earlier, keyword matching used raw substring `contains`, so a horror keyword like `оно` matched the substring
inside unrelated words such as `регионов` (a cooking show), and a single generic description word dragged in random
programmes. The fix:

- **Whole-word, Unicode-aware matching** — a keyword must be bounded by non-alphanumeric characters or string
  edges, so `оно` matches the title «Оно» but not `регионов`.
- **Distinctiveness / leading gate** — a title hit only counts as *strong* when the keyword is distinctive (a phrase
  or 8+ chars) or *leads* the title (so the Pixar film «Душа» matches `душа`, but "Тело и душа" doesn't).
- **Precision threshold** — a result qualifies only on a strong title hit or **≥2** distinct keyword hits anywhere;
  titles are weighted 3× over descriptions.

It's also genre-agnostic — there's no hard-coded genre list; the gate works the same for any query. See the test
`search_epg_excludes_substring_lone_desc_and_buried_common_word` in `tests/ai_test.rs`.

---

### The model keeps suggesting broad words and I get junk. What helps?

The prompt steers the model to emit **specific, recognizable titles** (Russian and original), not broad genre words,
and a stop-word list ("фильм", "сериал", "movie", "best", "top", …) is filtered out of any `KEYWORDS:` line before
searching. If you still get noise, ask for named titles ("какие фильмы Вильнёва идут") rather than a bare genre, or
add an instruction to your `ai_prompt.md` to always answer with concrete titles.

---

### Why don't HLS audio tracks show their dub names (just "aac 6ch")?

mpv's HLS demuxer does **not** propagate the `NAME=` attribute of `#EXT-X-MEDIA:TYPE=AUDIO` renditions, so movie/
IPTV providers that ship named dubs ("01. Дубляж (RUS)", "Оригинал", …) appear as several nameless `aac 6ch`
tracks. NEON-IPTV plays through your own mpv, so this is an mpv-side limitation, not the app.

A companion **mpv user script**, `hls_audio_names.lua`, fixes it: it fetches the HLS master manifest, parses the
`TYPE=AUDIO` rows, maps them to mpv track ids, collapses the per-quality duplication to distinct dub names, and
offers an OSD menu to switch audio by real name. It lives in your mpv config (`~/.config/mpv/scripts/`), not in this
repo, and is a pure local enhancement — every failure path is a logged no-op that leaves the stock track menu
working, and it never touches plain-http/IPTV streams with no named renditions.

---

### Why does playback buffer / stutter?

Most often it's the **provider serving the stream slower than its own encoded bitrate**, not the app. NEON-IPTV
already launches TV mpv with generous HLS buffering (`--cache=yes`, `--demuxer-max-bytes=1000MiB`,
`--hls-bitrate=max`, `http_persistent=1`, `--network-timeout=10`). Hardware decoding is deferred to your `mpv.conf`. To confirm where the
bottleneck is:

```bash
# Play the same URL directly and watch mpv's stats overlay (press i / I).
mpv --hls-bitrate=max --cache=yes "STREAM_URL"
# In the overlay compare "Video/Audio bitrate" vs the cache fill / network speed:
#   cache draining + speed < bitrate  →  the upstream is throttling (provider-side).
```

If a direct `mpv` plays just as badly, it's the source. If it plays fine but NEON-IPTV's window stutters, check your `~/.config/mpv/mpv.conf` to ensure proper hardware decoding (e.g., `hwdec=vaapi-copy` or `hwdec=auto`) is enabled for your GPU. Radio "buffering" is usually the station, not mpv.

---

### How do I force a full data reload / fix a broken cache?

Delete the cache and update again:

```bash
rm ~/.cache/neon-iptv/data.bin
ip_tv-neon update
```

The app self-heals: if `data.bin`'s version prefix doesn't match `CACHE_SCHEMA_VERSION`, or the file is too short or
fails to deserialize, it's removed automatically and rebuilt on the next update. A version mismatch is logged to
`neon_iptv.log` as `[cache] schema vX != expected vY — invalidating`.

---

### I rebuilt from newer source and channels vanished. Why?

If the developer bumped `CACHE_SCHEMA_VERSION` (because the bincode layout of `AppData`/`Channel`/`EpgProgram`/…
changed), your old `data.bin` is intentionally invalidated on first launch. Just run `ip_tv-neon update` once to
rebuild it. Your `config.json` (URLs, favorites, history, theme, AI provider) is a separate JSON file and is **not**
affected by a cache bump.

---

### If I add models, do I need to bump the cache version?

No. The selected model is stored in `config.json` as `llm_provider: String` (a catalog id token), which doesn't
change the bincode cache layout. Only changes to the cached data structs (`CacheContainer`, `AppData`, `Channel`,
`RadioStation`, `EpgProgram`) require incrementing `CACHE_SCHEMA_VERSION` in `src/consts.rs`.

---

### Is there a size limit on the EPG / playlist?

Yes — every download is bounded before allocation to defend against decompression bombs and OOM:

| Limit | Value |
|-------|-------|
| EPG download (compressed/raw) | 128 MiB |
| EPG after gzip | 768 MiB |
| EPG programmes | 4,000,000 |
| EPG channel names | 200,000 |
| Playlist download | 64 MiB |
| Channels per playlist | 500,000 |

The decompressed cap (768 MiB) has headroom over the default source (epg.one ≈ 417 MB decompressed). If a cap is
hit, parsing stops and `neon_iptv.log` records it (e.g. `[epg] decompressed-size cap … reached — EPG truncated`).
If your legitimate EPG is genuinely larger, raise the constant in `src/consts.rs` and rebuild.

---

### Why won't a `file://` (or other non-http) URL play?

By design. Only `http://` and `https://` media URLs reach mpv; anything else (`file://`, `edl://`, pipes, …) is
blocked **before** playback, so a crafted playlist can't make the player open a local file or pseudo-protocol. You'll
see `Blocked: only http/https media URLs are allowed` and a line in `neon_iptv.log`. To play a local *file*, use a
local **playlist** (a `.m3u` whose entries are http(s) streams) via the Local Playlists screen.

---

### Where are the logs?

| File | When |
|------|------|
| `~/.cache/neon-iptv/neon_iptv.log` | Always — errors, blocked URLs, cache/EPG-cap events |
| `$TMPDIR/neon_mpv.log` | Only with `--debug` — mpv's own log |
| `$TMPDIR/neon_mpv_stderr.log` | Radio playback stderr (always) |

`ip_tv-neon diag` prints the config/cache paths and current URLs without launching the TUI.

---

### Can I run it over SSH / in tmux / headless?

Yes for the TUI, EPG, AI chat and radio metadata — it's terminal-native. **Video** playback needs a display, because
TV streams open a real mpv window (`--force-window`). Over plain SSH with no display, audio/radio works but a TV
window has nowhere to render. Forward a display or run on the machine attached to the screen for video.

---

### Radio plays but the track/station name is wrong or missing.

Track info comes from two sources: the Radio Record "now playing" API (the green text in the station list) and live
mpv ICY metadata (`icy-name`, `icy-title`, `artist`/`title`) shown in the bottom player. If a station doesn't send
ICY metadata, the panel falls back to the playlist title and mpv's best-guess `media-title`. Note also that Radio
Record's `stream_320` label is misleading (it serves ~96 kbps), so the app uses `stream_128` as the real
max-quality stream.

---

### How do favorites and history work?

`f` toggles a favorite from the channel list or detail screen; favorites are a deduplicated set sorted by name.
History keeps the **last 200** watched stream URLs, most-recent first, deduplicated. Both persist in `config.json`,
along with a URL→name cache so they render even before a playlist is loaded. Clear either from Settings (*Clear
History* / *Clear Favorites*).

---

### How do I play an archived (past) programme?

On a channel with `tvg-rec` (archive) the detail screen shows past programmes marked `⏪`. Highlight one and press
`Enter`: the app builds a catchup URL (`STREAM?utc=<start>&lutc=<stop>`) and plays it. `l` always plays the live
edge regardless of selection. Channels without `tvg-rec` only play live.

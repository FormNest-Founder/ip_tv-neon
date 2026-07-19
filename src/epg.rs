// ─── Imports ─────────────────────────────────────────────────────────────────

use crate::consts::*;
use crate::models::{AppData, CacheContainer, Channel, Config, EpgProgram, RadioStation};
use crate::utils::{main_log, normalize, parse_xml_time, sanitize_terminal};
use anyhow::{Context, Result};
use chrono::Utc;
use flate2::read::GzDecoder;
use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read};
use std::path::PathBuf;
use std::sync::LazyLock;

// ─── M3U Regex Patterns ─────────────────────────────────────────────────────

static RE_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"tvg-id="([^"]+)""#).unwrap());
static RE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"tvg-name="([^"]+)""#).unwrap());
static RE_GROUP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"group-title="([^"]+)""#).unwrap());
static RE_REC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"tvg-rec="(\d+)""#).unwrap());

// ─── Bounded HTTP Body Reader ─────────────────────────────────────────────────

/// Stream an HTTP response body into memory with a hard byte cap (CG2). The
/// EPG/playlist URLs are user-editable and third-party, so the body is read in
/// chunks and rejected the moment it would exceed `cap` — no unbounded buffer.
async fn read_body_capped(mut resp: reqwest::Response, cap: usize, what: &str) -> Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .with_context(|| format!("{what}: reading body"))?
    {
        if buf.len().saturating_add(chunk.len()) > cap {
            anyhow::bail!("{what}: response exceeds {cap}-byte cap — refusing (possible bomb)");
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

// ─── Data Update (Radio + M3U + EPG) ─────────────────────────────────────────

pub async fn update_data(config: &Config, client: &reqwest::Client) -> Result<()> {
    main_log("Starting update_data...");

    let tracks_map = fetch_radio_now(client).await;
    let (radio, radio_genres) = fetch_radio_stations(client, &tracks_map, config).await;
    
    let mut r_groups: Vec<String> = radio_genres.into_iter().collect();
    r_groups.sort();

    let channels = fetch_playlist(client, config).await?;
    let groups: HashSet<String> = channels.iter().map(|ch| ch.group.clone()).collect();
    
    let mut sorted_groups: Vec<String> = groups.into_iter().collect();
    sorted_groups.sort();

    let mut group_counts: HashMap<String, usize> = HashMap::new();
    for ch in &channels {
        *group_counts.entry(ch.group.clone()).or_insert(0) += 1;
    }

    let (mut epg, name_to_id) = fetch_epg_data(client, config).await;

    for progs in epg.values_mut() {
        progs.sort_by_key(|p| p.start);
    }

    let data = AppData {
        channels,
        radio,
        radio_groups: r_groups,
        groups: sorted_groups,
        epg,
        name_to_id,
        group_counts,
        ..Default::default()
    };

    save_data(data)?;
    Ok(())
}

/// Fetch radio stations from API and local files
async fn fetch_radio_stations(
    client: &reqwest::Client,
    tracks_map: &HashMap<String, String>,
    config: &Config,
) -> (Vec<RadioStation>, HashSet<String>) {
    let mut radio = Vec::new();
    let mut radio_genres = HashSet::new();
    radio_genres.insert("All".to_string());

    if let Ok(r) = client.get(RADIO_API).send().await {
        if let Ok(j) = r.json::<serde_json::Value>().await {
            let stations = j["result"]["stations"]
                .as_array()
                .or_else(|| j["stations"].as_array());
            if let Some(st) = stations {
                for s in st {
                    let mut genres = Vec::new();
                    if let Some(gs) = s["genre"].as_array() {
                        for g in gs {
                            let g_name =
                                sanitize_terminal(g["name"].as_str().or(g.as_str()).unwrap_or(""))
                                    .to_uppercase();
                            if !g_name.is_empty() {
                                genres.push(g_name.clone());
                                radio_genres.insert(g_name);
                            }
                        }
                    }
                    let id_str = s["id"].as_i64().unwrap_or(0).to_string();

                    let mut quality_urls = HashMap::new();
                    quality_urls.insert(
                        "64".to_string(),
                        s["stream_64"].as_str().unwrap_or("").to_string(),
                    );
                    quality_urls.insert(
                        "128".to_string(),
                        s["stream_128"].as_str().unwrap_or("").to_string(),
                    );
                    quality_urls.insert(
                        "320".to_string(),
                        s["stream_320"].as_str().unwrap_or("").to_string(),
                    );
                    quality_urls.insert(
                        "hls".to_string(),
                        s["stream_hls"].as_str().unwrap_or("").to_string(),
                    );

                    radio.push(RadioStation {
                        id: id_str.clone(),
                        title: sanitize_terminal(s["title"].as_str().unwrap_or("")),
                        stream: s["stream_320"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .or_else(|| s["stream_128"].as_str().filter(|s| !s.is_empty()))
                            .or_else(|| s["stream_64"].as_str().filter(|s| !s.is_empty()))
                            .or_else(|| s["stream_hls"].as_str().filter(|s| !s.is_empty()))
                            .unwrap_or("")
                            .into(),
                        quality_urls,
                        genres,
                        provider: "Record".into(),
                        track: tracks_map.get(&id_str).cloned(),
                    });
                }
            }
        }
    }

    let local_files = scan_local_playlists(&config.local_dir);
    for path in local_files {
        if path.file_name().unwrap_or_default().to_string_lossy().to_lowercase().contains("radio") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let parsed = parse_m3u(&content);
                for ch in parsed {
                    let mut genres = Vec::new();
                    if !ch.group.is_empty() && ch.group != "Other" {
                        genres.push(ch.group.clone());
                        radio_genres.insert(ch.group.clone());
                    } else {
                        genres.push("Unknown".to_string());
                        radio_genres.insert("Unknown".to_string());
                    }
                    
                    radio.push(RadioStation {
                        id: ch.name.clone(),
                        title: ch.name.clone(),
                        stream: ch.url.clone(),
                        quality_urls: HashMap::new(),
                        genres,
                        provider: "Local".to_string(),
                        track: None,
                    });
                }
            }
        }
    }

    (radio, radio_genres)
}

/// Fetch and parse the main IPTV playlist
async fn fetch_playlist(client: &reqwest::Client, config: &Config) -> Result<Vec<Channel>> {
    let m3u = if config.playlist_url.starts_with("http") {
        let resp = client
            .get(&config.playlist_url)
            .send()
            .await
            .context("Failed to fetch playlist")?;
        let bytes = read_body_capped(resp, MAX_PLAYLIST_BYTES, "playlist").await?;
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        let path = config.playlist_url.clone();
        tokio::task::spawn_blocking(move || fs::read_to_string(&path))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking join: {e}"))?
            .context("Failed to read local playlist")?
    };

    Ok(parse_m3u(&m3u))
}

/// Fetch and parse the EPG data
async fn fetch_epg_data(
    client: &reqwest::Client,
    config: &Config,
) -> (HashMap<String, Vec<EpgProgram>>, HashMap<String, String>) {
    let mut epg: HashMap<String, Vec<EpgProgram>> = HashMap::new();
    let mut name_to_id: HashMap<String, String> = HashMap::new();

    match client.get(&config.epg_url).send().await {
        Ok(r) => match read_body_capped(r, MAX_EPG_DOWNLOAD_BYTES, "EPG").await {
            Ok(b) => {
                let epg_url_is_gz = config.epg_url.ends_with(".gz");
                let (parsed_epg, parsed_name_to_id) =
                    tokio::task::spawn_blocking(move || parse_epg(&b, epg_url_is_gz))
                        .await
                        .unwrap_or_default();
                epg = parsed_epg;
                name_to_id = parsed_name_to_id;
            }
            Err(e) => main_log(&format!("[epg] download rejected: {e}")),
        },
        Err(e) => main_log(&format!("[epg] fetch failed: {e}")),
    }

    (epg, name_to_id)
}
// ─── M3U Playlist Parser ──────────────────────────────────────────────────────

/// Parse an M3U/M3U8 playlist body into channels. `#EXTINF` lines carry the
/// tvg metadata + display name; the following `http(s)` line is the stream URL.
/// `#EXTGRP` may precede or follow its `#EXTINF`. Channels left without a URL
/// (malformed / trailing EXTINF) are dropped. Channel count is capped (CG2).
fn parse_m3u(m3u: &str) -> Vec<Channel> {
    let mut channels: Vec<Channel> = Vec::new();
    let mut cur_grp = "Other".to_string();
    let mut pending_grp: Option<String> = None;

    for line in m3u.lines() {
        let line = line.trim();
        if line.starts_with("#EXTGRP:") {
            let g = sanitize_terminal(line.trim_start_matches("#EXTGRP:").trim());
            if !g.is_empty() {
                // EXTGRP can appear before or after EXTINF — apply to last channel if pending
                if let Some(ch) = channels.last_mut() {
                    if ch.url.is_empty() {
                        // Channel was just added by EXTINF but has no URL yet — update its group
                        ch.group = g.clone();
                        cur_grp = g;
                        continue;
                    }
                }
                pending_grp = Some(g);
            }
        } else if line.starts_with("#EXTINF:") {
            if channels.len() >= MAX_CHANNELS {
                continue; // hard cap on channel count (CG2)
            }
            if let Some(g) = RE_GROUP.captures(line).map(|c| sanitize_terminal(&c[1])) {
                cur_grp = g;
            }
            if let Some(g) = pending_grp.take() {
                cur_grp = g;
            }

            let tid = RE_ID.captures(line).map(|c| c[1].to_string());
            let tname = RE_NAME.captures(line).map(|c| c[1].to_string());
            let rec_days = RE_REC
                .captures(line)
                .and_then(|c| c[1].parse().ok())
                .unwrap_or(0);
            let name = sanitize_terminal(line.rsplit(",").next().unwrap_or("").trim());
            channels.push(Channel {
                name_lower: name.to_lowercase(),
                name,
                group: cur_grp.clone(),
                url: "".into(),
                tvg_id: tid.or(tname),
                norm_name: normalize(""),
                catchup_days: rec_days,
            });
        } else if line.starts_with("http") {
            if let Some(ch) = channels.last_mut() {
                ch.url = line.to_string();
                ch.norm_name = normalize(&ch.name);
                ch.name_lower = ch.name.to_lowercase();
            }
        }
    }

    // Remove channels without URL
    channels.retain(|ch| !ch.url.is_empty());
    channels
}

// ─── EPG XMLTV Parser ─────────────────────────────────────────────────────────

/// Parse an XMLTV EPG payload (optionally gzip-compressed) into
/// (programmes-by-channel, normalized-display-name → channel-id).
///
/// Hardened against decompression bombs (CG2): the decompressed input fed to the
/// parser is capped at `MAX_EPG_DECOMPRESSED_BYTES`, the programme count at
/// `MAX_EPG_PROGRAMMES`, and the channel map at `MAX_EPG_CHANNELS`. Hitting the
/// programme cap logs loudly and stops parsing (CG5). Untrusted title/desc text
/// is sanitized at this parse boundary so every render site is safe (CG8).
fn parse_epg(
    b: &[u8],
    is_gz: bool,
) -> (HashMap<String, Vec<EpgProgram>>, HashMap<String, String>) {
    let mut epg_temp: HashMap<String, Vec<EpgProgram>> = HashMap::new();
    let mut name_to_id_temp: HashMap<String, String> = HashMap::new();
    let now = Utc::now().timestamp();
    let limit = now - 86400;

    let inner: Box<dyn Read + '_> = if is_gz || b.starts_with(&[0x1f, 0x8b]) {
        Box::new(GzDecoder::new(b))
    } else {
        Box::new(b)
    };
    let mut take = inner.take(MAX_EPG_DECOMPRESSED_BYTES);
    let mut reader = Reader::from_reader(BufReader::new(&mut take));
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut cur_id = String::new();
    let mut cur_prog: Option<EpgProgram> = None;
    let mut programme_count = 0usize;
    let mut dropped_count = 0usize;
    // When a programme has no (or an unparseable) stop time, assume it runs for
    // this long from its start rather than discarding it outright.
    const DEFAULT_SLOT_SECS: i64 = 3600;

    #[derive(PartialEq)]
    enum XmlTag {
        None,
        Title,
        Desc,
        DisplayName,
    }
    let mut tag = XmlTag::None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) => match e.name().as_ref() {
                b"channel" => {
                    cur_id = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"id")
                        .map(|a| String::from_utf8_lossy(&a.value).to_string())
                        .unwrap_or_default();
                    tag = XmlTag::None;
                }
                b"display-name" => {
                    tag = XmlTag::DisplayName;
                }
                b"programme" => {
                    let mut start = 0i64;
                    let mut stop = 0i64;
                    let mut ch_id = String::new();
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"start" => {
                                start = parse_xml_time(&String::from_utf8_lossy(&attr.value))
                            }
                            b"stop" => {
                                stop = parse_xml_time(&String::from_utf8_lossy(&attr.value))
                            }
                            b"channel" => {
                                ch_id = String::from_utf8_lossy(&attr.value).to_string()
                            }
                            _ => {}
                        }
                    }
                    // A missing/zero stop would otherwise drop the programme; if the
                    // start is valid, synthesize a default-length slot instead.
                    if stop == 0 && start > 0 {
                        stop = start + DEFAULT_SLOT_SECS;
                    }
                    if stop > limit && start > 0 {
                        cur_prog = Some(EpgProgram {
                            start,
                            stop,
                            title: String::new(),
                            desc: String::new(),
                        });
                        cur_id = ch_id;
                    } else {
                        dropped_count += 1;
                    }
                    tag = XmlTag::None;
                }
                b"title" => {
                    tag = XmlTag::Title;
                }
                b"desc" => {
                    tag = XmlTag::Desc;
                }
                _ => {} // unknown inner tags — don't overwrite current tag
            },
            Ok(XmlEvent::Text(e)) => {
                let text = e.unescape().unwrap_or_default().into_owned();
                match tag {
                    XmlTag::DisplayName => {
                        if name_to_id_temp.len() < MAX_EPG_CHANNELS {
                            name_to_id_temp.insert(normalize(&text), cur_id.clone());
                        }
                    }
                    XmlTag::Title => {
                        if let Some(p) = cur_prog.as_mut() {
                            p.title = sanitize_terminal(&text);
                        }
                    }
                    XmlTag::Desc => {
                        if let Some(p) = cur_prog.as_mut() {
                            p.desc = sanitize_terminal(&text);
                        }
                    }
                    XmlTag::None => {}
                }
            }
            Ok(XmlEvent::End(e)) => match e.name().as_ref() {
                b"programme" => {
                    if let Some(p) = cur_prog.take() {
                        epg_temp.entry(cur_id.clone()).or_default().push(p);
                        programme_count += 1;
                        if programme_count >= MAX_EPG_PROGRAMMES {
                            main_log(&format!(
                                "[epg] programme cap {MAX_EPG_PROGRAMMES} reached — truncating (possible bomb)"
                            ));
                            break;
                        }
                    }
                }
                b"title" | b"desc" | b"display-name" => {
                    tag = XmlTag::None;
                }
                _ => {}
            },
            Ok(XmlEvent::Eof) => break,
            Err(e) => {
                main_log(&format!("EPG XML parse error: {}", e));
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    drop(reader);
    // Loud truncation signal (CG5): a fully-consumed byte cap means the EPG was
    // cut short rather than reaching EOF, so the caller knows data is missing.
    if take.limit() == 0 {
        main_log(&format!(
            "[epg] decompressed-size cap {MAX_EPG_DECOMPRESSED_BYTES} reached — EPG truncated (possible bomb)"
        ));
    }
    if dropped_count > 0 {
        main_log(&format!(
            "[epg] dropped {dropped_count} programme(s) with invalid/old start or stop time"
        ));
    }
    (epg_temp, name_to_id_temp)
}

// ─── Cache Persistence ───────────────────────────────────────────────────────

pub fn save_data(data: AppData) -> Result<()> {
    let path = get_data_bin_path();
    fs::create_dir_all(get_cache_dir())?;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let f = File::create(&tmp)?;
    let writer = BufWriter::new(f);
    bincode::serialize_into(
        writer,
        &CacheContainer {
            version: CACHE_SCHEMA_VERSION,
            data,
        },
    )?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn load_data() -> AppData {
    let path = get_data_bin_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return AppData::default(),
    };

    if bytes.len() < 4 {
        main_log("[cache] file too short — invalidating");
        let _ = std::fs::remove_file(&path);
        return AppData::default();
    }

    let version = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    if version != CACHE_SCHEMA_VERSION {
        main_log(&format!(
            "[cache] schema v{version} != expected v{CACHE_SCHEMA_VERSION} — invalidating"
        ));
        let _ = std::fs::remove_file(&path);
        return AppData::default();
    }

    match bincode::deserialize::<CacheContainer>(&bytes) {
        Ok(c) if c.version == CACHE_SCHEMA_VERSION => {
            let mut data = c.data;
            data.build_indices();
            data
        }
        _ => {
            main_log("[cache] deserialize failed after version check — invalidating");
            let _ = std::fs::remove_file(&path);
            AppData::default()
        }
    }
}

// ─── Radio Now Playing ───────────────────────────────────────────────────────

pub async fn fetch_radio_now(client: &reqwest::Client) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(r) = client.get(RADIO_NOW_API).send().await {
        if let Ok(j) = r.json::<serde_json::Value>().await {
            // API returns {"result": [...]} or bare [...]
            let stations = j["result"].as_array().or_else(|| j.as_array());
            if let Some(st) = stations {
                for s in st {
                    let id = s["id"].as_i64().unwrap_or(0).to_string();
                    let mut artist = s["track"]["artist"].as_str().unwrap_or("").trim().to_string();
                    let mut song = s["track"]["song"].as_str().unwrap_or("").trim().to_string();
                    
                    // FILTER: Misconfigured RadioRecord stations sometimes return double-encoded JSON
                    // (a backend response string) in the artist field instead of the actual artist.
                    if artist.starts_with('{') && artist.ends_with('}') {
                        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&artist) {
                            let track_obj = j.get("result").or(j.get("track")).unwrap_or(&j);
                            let ext_a = track_obj.get("artist").and_then(|v| v.as_str()).unwrap_or("");
                            let ext_s = track_obj.get("song").or(track_obj.get("title")).and_then(|v| v.as_str()).unwrap_or("");
                            if !ext_a.is_empty() || !ext_s.is_empty() {
                                artist = ext_a.to_string();
                                song = ext_s.to_string();
                            } else {
                                artist.clear(); // Clear the junk JSON if we couldn't parse it
                            }
                        } else {
                            artist.clear();
                        }
                    }

                    if !artist.is_empty() {
                        map.insert(id, sanitize_terminal(&format!("{} - {}", artist, song)));
                    } else if !song.is_empty() {
                        map.insert(id, sanitize_terminal(&song));
                    }
                }
            }
        }
    }
    map
}

// ─── EPG Lookup Helpers ──────────────────────────────────────────────────────

pub fn find_epg_id(ch: &Channel, data: &AppData) -> Option<String> {
    if let Some(id) = &ch.tvg_id {
        if data.epg.contains_key(id) {
            return Some(id.clone());
        }
    }
    data.name_to_id.get(&ch.norm_name).cloned()
}

/// Return the programme airing at `now` on `ch`, if any.
///
/// The per-channel programme list is sorted by `start` (see `update_data`), so a
/// binary `partition_point` finds the last programme that started at or before
/// `now` in O(log n) instead of a linear scan. That candidate is the only one
/// that can contain `now` in a well-formed, non-overlapping schedule; it is
/// returned by reference to avoid cloning on every render.
pub fn get_current_epg<'a>(ch: &Channel, data: &'a AppData, now: i64) -> Option<&'a EpgProgram> {
    let id = find_epg_id(ch, data)?;
    let progs = data.epg.get(&id)?;
    let idx = progs.partition_point(|p| p.start <= now);
    let p = progs.get(idx.checked_sub(1)?)?;
    (now >= p.start && now < p.stop).then_some(p)
}

// ─── Local Playlist Scanner ──────────────────────────────────────────────────

pub fn scan_local_playlists(custom_dir: &str) -> Vec<PathBuf> {
    let mut res = Vec::new();
    let scan_dirs: Vec<PathBuf> = if custom_dir.is_empty() {
        [dirs::home_dir(), dirs::download_dir(), dirs::video_dir()]
            .into_iter()
            .flatten()
            .collect()
    } else {
        vec![PathBuf::from(custom_dir)]
    };
    for dir in scan_dirs {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension()
                    .is_some_and(|ext| ext == "m3u" || ext == "m3u8")
                {
                    res.push(p);
                }
            }
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::{parse_epg, parse_m3u};

    // ── M3U ──────────────────────────────────────────────────────────────

    #[test]
    fn m3u_wellformed_parses_metadata() {
        let m3u = "\
#EXTM3U
#EXTINF:-1 tvg-id=\"bbc.one\" group-title=\"News\" tvg-rec=\"7\",BBC One
http://host/bbc1
#EXTINF:-1,Movie Ch
http://host/movie
";
        let chans = parse_m3u(m3u);
        assert_eq!(chans.len(), 2);
        let bbc = &chans[0];
        assert_eq!(bbc.name, "BBC One");
        assert_eq!(bbc.group, "News");
        assert_eq!(bbc.tvg_id.as_deref(), Some("bbc.one"));
        assert_eq!(bbc.catchup_days, 7);
        assert_eq!(bbc.url, "http://host/bbc1");
        assert_eq!(bbc.name_lower, "bbc one");
    }

    #[test]
    fn m3u_drops_extinf_without_url() {
        let m3u = "\
#EXTM3U
#EXTINF:-1,Orphan No URL
#EXTINF:-1,Has URL
http://host/ok
garbage line that is not a directive
";
        let chans = parse_m3u(m3u);
        assert_eq!(chans.len(), 1);
        assert_eq!(chans[0].name, "Has URL");
    }

    #[test]
    fn m3u_extgrp_before_extinf_sets_group() {
        let m3u = "\
#EXTM3U
#EXTGRP:Movies
#EXTINF:-1,Film
http://host/film
";
        let chans = parse_m3u(m3u);
        assert_eq!(chans.len(), 1);
        assert_eq!(chans[0].group, "Movies");
    }

    #[test]
    fn m3u_empty_input_yields_no_channels() {
        assert!(parse_m3u("").is_empty());
        assert!(parse_m3u("not a playlist at all").is_empty());
    }

    // ── XMLTV EPG ────────────────────────────────────────────────────────

    #[test]
    fn epg_wellformed_parses_programme_and_channel_name() {
        // Far-future times so they survive the "older than 1 day" cutoff.
        let xml = "\
<tv>
  <channel id=\"ch1\"><display-name>BBC One</display-name></channel>
  <programme start=\"20991231120000 +0000\" stop=\"20991231130000 +0000\" channel=\"ch1\">
    <title>Evening News</title>
    <desc>The day in review</desc>
  </programme>
</tv>";
        let (epg, name_to_id) = parse_epg(xml.as_bytes(), false);
        let progs = epg.get("ch1").expect("ch1 present");
        assert_eq!(progs.len(), 1);
        assert_eq!(progs[0].title, "Evening News");
        assert_eq!(progs[0].desc, "The day in review");
        assert_eq!(progs[0].stop - progs[0].start, 3600);
        // display-name is normalized (lowercase alphanumerics) → channel id
        assert_eq!(name_to_id.get("bbcone").map(String::as_str), Some("ch1"));
    }

    #[test]
    fn epg_missing_stop_gets_default_slot() {
        let xml = "\
<tv>
  <programme start=\"20991231120000 +0000\" channel=\"ch1\">
    <title>No Stop Time</title>
  </programme>
</tv>";
        let (epg, _) = parse_epg(xml.as_bytes(), false);
        let progs = epg.get("ch1").expect("ch1 present");
        assert_eq!(progs.len(), 1);
        // Synthesized: stop == start + 3600 (DEFAULT_SLOT_SECS).
        assert_eq!(progs[0].stop - progs[0].start, 3600);
    }

    #[test]
    fn epg_minutes_only_time_is_kept() {
        let xml = "\
<tv>
  <programme start=\"209912311200 +0000\" stop=\"209912311300 +0000\" channel=\"ch1\">
    <title>Minutes Only</title>
  </programme>
</tv>";
        let (epg, _) = parse_epg(xml.as_bytes(), false);
        assert_eq!(epg.get("ch1").map(Vec::len), Some(1));
    }

    #[test]
    fn epg_garbage_start_is_dropped() {
        let xml = "\
<tv>
  <programme start=\"not-a-time\" stop=\"also-bad\" channel=\"ch2\">
    <title>Junk</title>
  </programme>
</tv>";
        let (epg, _) = parse_epg(xml.as_bytes(), false);
        assert!(!epg.contains_key("ch2"));
    }

    #[test]
    fn epg_empty_input_is_empty() {
        let (epg, name_to_id) = parse_epg(b"", false);
        assert!(epg.is_empty());
        assert!(name_to_id.is_empty());
    }
}

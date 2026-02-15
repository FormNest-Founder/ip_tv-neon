use crate::models::{AppData, CacheContainer, Channel, Config, EpgProgram, RadioStation};
use crate::utils::{main_log, normalize, parse_xml_time};
use crate::consts::*;
use anyhow::{Context, Result};
use chrono::Utc;
use flate2::read::GzDecoder;
use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

static RE_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"tvg-id="([^"]+)""#).unwrap());
static RE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"tvg-name="([^"]+)""#).unwrap());
static RE_GROUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"group-title="([^"]+)""#).unwrap());

pub async fn update_data(config: &Config) -> Result<()> {
    main_log("Starting update_data...");
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(15))
        .build()?;

    let tracks_map = fetch_radio_now(&client).await;
    let mut radio = Vec::new();
    let mut radio_genres = HashSet::new();
    radio_genres.insert("All".to_string());

    if let Ok(r) = client.get(RADIO_API).send().await {
        if let Ok(j) = r.json::<serde_json::Value>().await {
            let stations = j["result"]["stations"].as_array().or_else(|| j["stations"].as_array());
            if let Some(st) = stations {
                for s in st {
                    let mut genres = Vec::new();
                    if let Some(gs) = s["genre"].as_array() {
                        for g in gs {
                            let g_name = g["name"].as_str().or(g.as_str()).unwrap_or("").to_uppercase();
                            if !g_name.is_empty() {
                                genres.push(g_name.clone());
                                radio_genres.insert(g_name);
                            }
                        }
                    }
                    let id_str = s["id"].as_i64().unwrap_or(0).to_string();
                    radio.push(RadioStation {
                        id: id_str.clone(),
                        title: s["title"].as_str().unwrap_or("").into(),
                        stream: s["stream_320"].as_str().or(s["stream_hls"].as_str()).unwrap_or("").into(),
                        genres,
                        provider: "Record".into(),
                        track: tracks_map.get(&id_str).cloned(),
                    });
                }
            }
        }
    }

    let mut r_groups: Vec<String> = radio_genres.into_iter().collect();
    r_groups.sort();

    let m3u = if config.playlist_url.starts_with("http") {
        client.get(&config.playlist_url).send().await?.text().await.context("Failed to fetch playlist")?
    } else {
        fs::read_to_string(&config.playlist_url).context("Failed to read local playlist")?
    };

    let mut channels = Vec::new();
    let mut groups = HashSet::new();
    let mut cur_grp = "Other".to_string();
    let mut pending_grp: Option<String> = None;

    for line in m3u.lines() {
        let line = line.trim();
        if line.starts_with("#EXTGRP:") {
            // Format: #EXTGRP:GroupName (separate line before URL)
            let g = line.trim_start_matches("#EXTGRP:").trim().to_string();
            if !g.is_empty() {
                pending_grp = Some(g);
            }
        } else if line.starts_with("#EXTINF:") {
            let tid = RE_ID.captures(line).map(|c| c[1].to_string());
            let tname = RE_NAME.captures(line).map(|c| c[1].to_string());
            // group-title="..." inside #EXTINF (standard format)
            if let Some(g) = RE_GROUP.captures(line).map(|c| c[1].to_string()) {
                cur_grp = g;
                groups.insert(cur_grp.clone());
            }
            // #EXTGRP on previous line overrides
            if let Some(g) = pending_grp.take() {
                cur_grp = g;
                groups.insert(cur_grp.clone());
            }
            let name = line.rsplit(",").next().unwrap_or("").trim().to_string();
            channels.push(Channel {
                name: name.clone(),
                group: cur_grp.clone(),
                url: "".into(),
                tvg_id: tid.or(tname),
                norm_name: normalize(&name),
                catchup_days: 0,
            });
        } else if line.starts_with("http") {
            // #EXTGRP can also appear between #EXTINF and URL
            if let Some(g) = pending_grp.take() {
                cur_grp = g.clone();
                groups.insert(cur_grp.clone());
                if let Some(ch) = channels.last_mut() { ch.group = cur_grp.clone(); }
            }
            if let Some(ch) = channels.last_mut() { ch.url = line.to_string(); }
        }
    }

    let mut epg: HashMap<String, Vec<EpgProgram>> = HashMap::new();
    let mut name_to_id: HashMap<String, String> = HashMap::new();
    let now = Utc::now().timestamp();
    let limit = now - 86400;

    if let Ok(r) = client.get(&config.epg_url).send().await {
        if let Ok(b) = r.bytes().await {
            let reader_raw: Box<dyn BufRead> = if config.epg_url.ends_with(".gz") || b.starts_with(&[0x1f, 0x8b]) {
                Box::new(BufReader::new(GzDecoder::new(&b[..])))
            } else {
                Box::new(BufReader::new(&b[..]))
            };
            let mut reader = Reader::from_reader(reader_raw);
            reader.trim_text(true);
            let mut buf = Vec::new();
            let mut cur_id = String::new();
            let mut cur_prog: Option<EpgProgram> = None;
            let mut tag = String::new();

            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(XmlEvent::Start(e)) => {
                        tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                        match tag.as_str() {
                            "channel" => { cur_id = e.attributes().flatten().find(|a| a.key.as_ref() == b"id").map(|a| String::from_utf8_lossy(&a.value).to_string()).unwrap_or_default(); }
                            "display-name" => { }
                            "programme" => {
                                let start = e.attributes().flatten().find(|a| a.key.as_ref() == b"start").map(|a| parse_xml_time(&String::from_utf8_lossy(&a.value))).unwrap_or(0);
                                let stop = e.attributes().flatten().find(|a| a.key.as_ref() == b"stop").map(|a| parse_xml_time(&String::from_utf8_lossy(&a.value))).unwrap_or(0);
                                let ch_id = e.attributes().flatten().find(|a| a.key.as_ref() == b"channel").map(|a| String::from_utf8_lossy(&a.value).to_string()).unwrap_or_default();
                                if stop > limit {
                                    cur_prog = Some(EpgProgram { start, stop, title: "".into(), desc: "".into() });
                                    cur_id = ch_id;
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(XmlEvent::Text(e)) => {
                        let text = e.unescape().unwrap_or_default().into_owned();
                        match tag.as_str() {
                            "display-name" => { name_to_id.insert(normalize(&text), cur_id.clone()); }
                            "title" => { if let Some(p) = cur_prog.as_mut() { p.title = text; } }
                            "desc" => { if let Some(p) = cur_prog.as_mut() { p.desc = text; } }
                            _ => {}
                        }
                    }
                    Ok(XmlEvent::End(e)) => {
                        if e.name().as_ref() == b"programme" {
                            if let Some(p) = cur_prog.take() {
                                epg.entry(cur_id.clone()).or_default().push(p);
                            }
                        }
                        tag.clear();
                    }
                    Ok(XmlEvent::Eof) => break,
                    Err(e) => { main_log(&format!("EPG XML parse error: {}", e)); break; }
                    _ => {}
                }
                buf.clear();
            }
        }
    }

    for progs in epg.values_mut() {
        progs.sort_by_key(|p| p.start);
    }

    let mut sorted_groups: Vec<String> = groups.into_iter().collect();
    sorted_groups.sort();

    let data = AppData {
        channels,
        radio,
        radio_groups: r_groups,
        groups: sorted_groups,
        epg,
        name_to_id,
    };

    save_data(data)?;
    Ok(())
}

pub fn save_data(data: AppData) -> Result<()> {
    let path = get_data_bin_path();
    fs::create_dir_all(get_cache_dir())?;
    let tmp = path.with_extension("tmp");
    let f = File::create(&tmp)?;
    let writer = BufWriter::new(f);
    bincode::serialize_into(writer, &CacheContainer { version: APP_VERSION, data })?;
    fs::rename(tmp, path)?;
    Ok(())
}

async fn fetch_radio_now(client: &reqwest::Client) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(r) = client.get(RADIO_NOW_API).send().await {
        if let Ok(j) = r.json::<serde_json::Value>().await {
            // API returns {"result": [...]} or bare [...]
            let stations = j["result"].as_array().or_else(|| j.as_array());
            if let Some(st) = stations {
                for s in st {
                    let id = s["id"].as_i64().unwrap_or(0).to_string();
                    let artist = s["track"]["artist"].as_str().unwrap_or("");
                    let song = s["track"]["song"].as_str().unwrap_or("");
                    if !artist.is_empty() { map.insert(id, format!("{} - {}", artist, song)); }
                }
            }
        }
    }
    map
}

pub fn find_epg_id(ch: &Channel, data: &AppData) -> Option<String> {
    if let Some(id) = &ch.tvg_id { if data.epg.contains_key(id) { return Some(id.clone()); } }
    data.name_to_id.get(&ch.norm_name).cloned()
}

pub fn get_current_epg(ch: &Channel, data: &AppData, now: i64) -> Option<EpgProgram> {
    let id = find_epg_id(ch, data)?;
    data.epg.get(&id)?.iter().find(|p| now >= p.start && now < p.stop).cloned()
}

pub fn scan_local_playlists() -> Vec<PathBuf> {
    let mut res = Vec::new();
    if let Ok(entries) = fs::read_dir(".") {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|ext| ext == "m3u" || ext == "m3u8") { res.push(p); }
        }
    }
    res
}

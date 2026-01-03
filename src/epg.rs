use anyhow::Result;
use chrono::Utc;
use flate2::read::GzDecoder;
use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Duration;

use crate::models::{AppData, Channel, Config, EpgProgram, RadioStation};
use crate::utils::{
    get_cache_dir, main_log, normalize, parse_xml_time, RADIO_API, RADIO_NOW_API, UA,
};

pub async fn update_data(config: &Config) -> Result<()> {
    main_log("Starting update_data...");
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(15))
        .build()?;

    let tracks_map = fetch_radio_now().await;

    main_log("Fetching Radio Record stations...");
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
                            let g_name = if let Some(name) = g["name"].as_str() {
                                name.to_uppercase()
                            } else if let Some(name) = g.as_str() {
                                name.to_uppercase()
                            } else {
                                continue;
                            };
                            genres.push(g_name.clone());
                            radio_genres.insert(g_name);
                        }
                    }
                    if genres.is_empty() {
                        genres.push("ELECTRONIC".to_string());
                        radio_genres.insert("ELECTRONIC".to_string());
                    }

                    let id_str = s["id"].as_i64().unwrap_or(0).to_string();
                    let current_track = tracks_map.get(&id_str).cloned();

                    radio.push(RadioStation {
                        id: id_str,
                        title: s["title"].as_str().unwrap_or("").into(),
                        stream: s["stream_hls"]
                            .as_str()
                            .or(s["stream_320"].as_str())
                            .unwrap_or("")
                            .into(),
                        genres,
                        provider: "Record".into(),
                        track: current_track,
                    });
                }
            }
        }
    }

    main_log("Adding Zaycev.fm stations...");
    let zaycev_data = vec![
        (
            "z_pop",
            "Zaycev POP",
            "https://abs.zaycev.fm/pop128k",
            vec!["POP"],
        ),
        (
            "z_rock",
            "Zaycev ROCK",
            "https://abs.zaycev.fm/rock128k",
            vec!["ROCK"],
        ),
        (
            "z_club",
            "Zaycev CLUB",
            "https://abs.zaycev.fm/club128k",
            vec!["CLUB", "ELECTRONIC"],
        ),
        (
            "z_disco",
            "Zaycev DISCO",
            "https://abs.zaycev.fm/disco128k",
            vec!["DISCO", "OLD SCHOOL"],
        ),
        (
            "z_relax",
            "Zaycev RELAX",
            "https://abs.zaycev.fm/relax128k",
            vec!["RELAX", "CHILL"],
        ),
        (
            "z_alternative",
            "Zaycev METAL",
            "https://abs.zaycev.fm/metal128k",
            vec!["ROCK", "METAL"],
        ),
        (
            "z_rap",
            "Zaycev RAP",
            "https://abs.zaycev.fm/rap128k",
            vec!["RAP", "HIP-HOP"],
        ),
        (
            "z_trap",
            "Zaycev BASS",
            "https://abs.zaycev.fm/bass128k",
            vec!["TRAP", "ELECTRONIC"],
        ),
        (
            "z_chanson",
            "Zaycev CHANSON",
            "https://abs.zaycev.fm/shanson128k",
            vec!["CHANSON"],
        ),
    ];

    for (id, title, stream, genres) in zaycev_data {
        let mut gs = Vec::new();
        for g in genres {
            let gn = g.to_string();
            gs.push(gn.clone());
            radio_genres.insert(gn);
        }
        radio.push(RadioStation {
            id: id.into(),
            title: title.into(),
            stream: stream.into(),
            genres: gs,
            provider: "Zaycev".into(),
            track: None,
        });
    }

    let mut r_groups: Vec<String> = radio_genres.into_iter().collect();
    r_groups.sort();
    if let Some(pos) = r_groups.iter().position(|x| x == "All") {
        r_groups.remove(pos);
        r_groups.insert(0, "All".to_string());
    }

    main_log("Fetching playlist...");
    let m3u_res = if config.playlist_url.starts_with("http") {
        let url = config.playlist_url.clone();
        let fut = client.get(&url).send();
        match tokio::time::timeout(Duration::from_secs(15), fut).await {
            Ok(Ok(resp)) => {
                let bytes = resp.bytes().await?;
                Ok(String::from_utf8_lossy(&bytes).to_string())
            }
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err(anyhow::anyhow!("Playlist timeout")),
        }
    } else {
        std::fs::read_to_string(&config.playlist_url).map_err(|e| e.into())
    };

    let m3u = m3u_res.unwrap_or_default();

    main_log("Parsing channels...");
    let mut channels = Vec::new();
    let mut groups = HashSet::new();
    let mut cur_grp = "Other".to_string();
    let re_id = Regex::new(r#"tvg-id="([^"]+)""#).unwrap();
    let re_name = Regex::new(r#"tvg-name="([^"]+)""#).unwrap();
    let re_group = Regex::new(r#"group-title="([^"]+)""#).unwrap();

    for line in m3u.lines() {
        if line.starts_with("#EXTINF:") {
            let tid = re_id.captures(line).map(|c| c[1].to_string());
            let tname = re_name.captures(line).map(|c| c[1].to_string());
            if let Some(g) = re_group.captures(line).map(|c| c[1].to_string()) {
                cur_grp = g;
                groups.insert(cur_grp.clone());
            }
            let name = line.split(',').next_back().unwrap_or("").trim().to_string();
            channels.push(Channel {
                name,
                group: cur_grp.clone(),
                url: "".into(),
                tvg_id: tid.or(tname),
                norm_name: normalize(line.split(',').next_back().unwrap_or("")),
                catchup_days: 0,
            });
        } else if let Some(g) = line.strip_prefix("#EXTGRP:") {
            cur_grp = g.trim().to_string();
            groups.insert(cur_grp.clone());
        } else if line.starts_with("http") {
            if let Some(ch) = channels.last_mut() {
                ch.url = line.to_string();
            }
        }
    }

    main_log("Fetching EPG...");
    let mut epg: HashMap<String, Vec<EpgProgram>> = HashMap::new();
    let mut name_to_id: HashMap<String, String> = HashMap::new();
    let mut prog_count = 0;

    if let Ok(r) = client.get(&config.epg_url).send().await {
        let b = r.bytes().await?;
        let reader_raw: Box<dyn BufRead> =
            if config.epg_url.ends_with(".gz") || b.starts_with(&[0x1f, 0x8b]) {
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
        let now = Utc::now().timestamp();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(XmlEvent::Start(e)) => {
                    tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if tag == "channel" {
                        if let Some(a) = e.attributes().flatten().find(|a| a.key.as_ref() == b"id")
                        {
                            cur_id = String::from_utf8_lossy(&a.value).into();
                        }
                    } else if tag == "programme" {
                        let (mut start, mut stop, mut cid) = (0, 0, String::new());
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"start" => {
                                    start = parse_xml_time(&String::from_utf8_lossy(&a.value))
                                }
                                b"stop" => {
                                    stop = parse_xml_time(&String::from_utf8_lossy(&a.value))
                                }
                                b"channel" => cid = String::from_utf8_lossy(&a.value).into(),
                                _ => {}
                            }
                        }
                        if stop > now - 86400 {
                            cur_prog = Some(EpgProgram {
                                start,
                                stop,
                                title: "".into(),
                                desc: "".into(),
                            });
                            cur_id = cid;
                        }
                    }
                }
                Ok(XmlEvent::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().into_owned();
                    if tag == "display-name" {
                        name_to_id.insert(normalize(&text), cur_id.clone());
                    }
                    if let Some(p) = cur_prog.as_mut() {
                        if tag == "title" {
                            p.title = text;
                        } else if tag == "desc" {
                            p.desc = text;
                        }
                    }
                }
                Ok(XmlEvent::End(e)) => {
                    if e.name().as_ref() == b"programme" {
                        if let Some(p) = cur_prog.take() {
                            epg.entry(cur_id.clone()).or_default().push(p);
                            prog_count += 1;
                        }
                    }
                }
                Ok(XmlEvent::Eof) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    main_log(&format!("EPG Parsed: {} programs loaded.", prog_count));

    let mut g_vec: Vec<String> = groups.into_iter().collect();
    g_vec.sort();
    let data = AppData {
        channels,
        radio,
        radio_groups: r_groups,
        groups: g_vec,
        epg,
        name_to_id,
    };
    let cache_dir = get_cache_dir();
    let _ = fs::create_dir_all(&cache_dir);
    bincode::serialize_into(File::create(cache_dir.join("data.bin"))?, &data)?;
    main_log("update_data finished.");
    Ok(())
}

pub async fn fetch_radio_now() -> HashMap<String, String> {
    let client = reqwest::Client::builder().user_agent(UA).build().unwrap();
    let mut map = HashMap::new();
    if let Ok(r) = client.get(RADIO_NOW_API).send().await {
        if let Ok(j) = r.json::<serde_json::Value>().await {
            let res_arr = j["result"].as_array().or_else(|| j.as_array());
            if let Some(res) = res_arr {
                for st in res {
                    let id = st["id"].as_i64().unwrap_or(0).to_string();
                    let artist = st["track"]["artist"].as_str().unwrap_or("");
                    let song = st["track"]["song"].as_str().unwrap_or("");
                    if !artist.is_empty() {
                        map.insert(id, format!("{} - {}", artist, song));
                    }
                }
            }
        }
    }
    map
}

pub fn find_epg_id(ch: &Channel, data: &AppData) -> Option<String> {
    if let Some(id) = &ch.tvg_id {
        if data.epg.contains_key(id) {
            return Some(id.clone());
        }
    }
    data.name_to_id.get(&ch.norm_name).cloned()
}

pub fn get_current_epg<'a>(ch: &Channel, data: &'a AppData, now: i64) -> Option<&'a EpgProgram> {
    let id = find_epg_id(ch, data)?;
    data.epg
        .get(&id)
        .and_then(|progs| progs.iter().find(|p| p.start <= now && p.stop > now))
}

pub fn scan_local_playlists() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let dirs_to_scan = vec![
        dirs::download_dir(),
        dirs::video_dir(),
        Some(PathBuf::from("/mnt")),
        Some(PathBuf::from("/media")),
    ];

    for dir in dirs_to_scan.into_iter().flatten() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if ext.eq_ignore_ascii_case("m3u") || ext.eq_ignore_ascii_case("m3u8") {
                            files.push(path);
                        }
                    }
                }
            }
        }
    }
    files.sort();
    files
}

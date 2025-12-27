use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::io::Read;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use flate2::read::GzDecoder;
use chrono::{Local, NaiveDateTime};
use clap::{Parser, Subcommand, Args};
use regex::Regex;

const PLAYLIST_URL: &str = "http://331273bff393.goodstreem.org/playlists/uplist/bc17084cb401b17401e1001e4c4cb80a/playlist.m3u8";
const EPG_URL: &str = "http://epg.it999.ru/edem.xml.gz";
const CACHE_DIR: &str = "/tmp/neon_iptv_rs";

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Groups,
    Channels { group: String },
    Play(PlayArgs),
    Update,
}

#[derive(Args)]
struct PlayArgs {
    #[arg(long)]
    url: String,
    #[arg(long)]
    title: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Channel {
    name: String,
    tvg_name: String,
    group: String,
    url: String,
    epg_now: Option<String>,
}

fn get_cache_paths() -> (PathBuf, PathBuf) {
    let dir = Path::new(CACHE_DIR);
    if !dir.exists() { fs::create_dir_all(dir).ok(); }
    (dir.join("epg.xml"), dir.join("data.json"))
}

async fn download_file(url: &str, path: &Path, is_gz: bool) -> Result<()> {
    let resp = reqwest::get(url).await?.bytes().await?;
    if is_gz {
        let mut decoder = GzDecoder::new(&resp[..]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded)?;
        fs::write(path, decoded)?;
    } else {
        fs::write(path, resp)?;
    }
    Ok(())
}

fn normalize_name(name: &str) -> String {
    let name = name.to_uppercase();
    let re = Regex::new(r"(?i)\(.*\)|HD|FHD|UHD|SD|4K|RU|BY|UA|KAZ|UZB|EST|LAT|LIT|PL|DE|FR|EN|ORIGIN|V\.2|V\.3|\+").unwrap();
    let cleaned = re.replace_all(&name, "");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_epg_time(time_str: &str) -> i64 {
    if time_str.len() < 14 { return 0; }
    let base = &time_str[0..14];
    if let Ok(naive) = NaiveDateTime::parse_from_str(base, "%Y%m%d%H%M%S") {
        if time_str.len() >= 20 {
            let offset_part = &time_str[15..20];
            if let Ok(hours) = offset_part[0..3].parse::<i64>() {
                return naive.and_utc().timestamp() - hours * 3600;
            }
        }
        return naive.and_utc().timestamp();
    }
    0
}

fn parse_epg(path: &Path) -> std::collections::HashMap<String, String> {
    let mut epg_map = std::collections::HashMap::new();
    let file = fs::File::open(path).ok();
    if file.is_none() { return epg_map; }
    let mut reader = Reader::from_reader(std::io::BufReader::new(file.unwrap()));
    reader.config_mut().trim_text(true);
    
    let now = Local::now().timestamp();
    let mut channel_id_to_name = std::collections::HashMap::new();
    let mut buf = Vec::new();
    
    let mut current_channel_id = String::new();
    let mut in_display_name = false;
    let mut in_programme_title = false;
    let mut programme_channel_id = String::new();
    let mut skip_programme = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                match e.name().as_ref() {
                    b"channel" => {
                        current_channel_id = e.attributes().filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"id")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                            .unwrap_or_default();
                    }
                    b"display-name" => in_display_name = true,
                    b"programme" => {
                        let start_str = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"start")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                        let stop_str = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"stop")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                        
                        let start = parse_epg_time(&start_str);
                        let stop = parse_epg_time(&stop_str);
                        
                        if start <= now && now <= stop {
                            programme_channel_id = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"channel")
                                .map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                            skip_programme = false;
                        } else {
                            skip_programme = true;
                        }
                    }
                    b"title" => {
                        if !skip_programme && !programme_channel_id.is_empty() {
                            in_programme_title = true;
                        }
                    }
                    _ => (),
                }
            }
            Ok(Event::Text(e)) => {
                let text = String::from_utf8_lossy(e.as_ref()).into_owned();
                if in_display_name && !current_channel_id.is_empty() {
                    channel_id_to_name.insert(current_channel_id.clone(), normalize_name(&text));
                } else if in_programme_title {
                    if let Some(norm_name) = channel_id_to_name.get(&programme_channel_id) {
                        epg_map.insert(norm_name.clone(), text);
                    }
                }
            }
            Ok(Event::End(e)) => {
                match e.name().as_ref() {
                    b"display-name" => in_display_name = false,
                    b"title" => in_programme_title = false,
                    b"channel" => current_channel_id.clear(),
                    b"programme" => {
                        programme_channel_id.clear();
                        skip_programme = false;
                    }
                    _ => (),
                }
            }
            Ok(Event::Eof) => break,
            _ => (),
        }
        buf.clear();
    }
    epg_map
}

async fn update_data() -> Result<()> {
    let (epg_path, json_path) = get_cache_paths();
    println!("Updating EPG...");
    download_file(EPG_URL, &epg_path, true).await?;
    let epg_data = parse_epg(&epg_path);

    println!("Updating Playlist...");
    let m3u_resp = reqwest::get(PLAYLIST_URL).await?.text().await?;
    let mut channels = Vec::new();
    let mut groups = std::collections::HashSet::new();

    let mut current_name = String::new();
    let mut current_tvg_name = String::new();
    let mut current_group = String::new();
    
    let re_tvg = Regex::new(r#"tvg-name="([^"]+)""#).unwrap();

    for line in m3u_resp.lines() {
        if line.starts_with("#EXTINF:") {
            if let Some(caps) = re_tvg.captures(line) {
                current_tvg_name = caps.get(1).unwrap().as_str().to_string();
            }
            if let Some(pos) = line.rfind(',') {
                current_name = line[pos+1..].trim().to_string();
                if current_tvg_name.is_empty() { current_tvg_name = current_name.clone(); }
            }
        } else if line.starts_with("#EXTGRP:") {
            current_group = line[8..].trim().to_string();
        } else if line.starts_with("http") {
            if current_group.is_empty() { current_group = "Other".to_string(); }
            groups.insert(current_group.clone());
            
            let norm_tvg = normalize_name(&current_tvg_name);
            let norm_name = normalize_name(&current_name);
            let prog = epg_data.get(&norm_tvg).or_else(|| epg_data.get(&norm_name)).cloned();
            
            channels.push(Channel {
                name: current_name.clone(),
                tvg_name: current_tvg_name.clone(),
                group: current_group.clone(),
                url: line.trim().to_string(),
                epg_now: prog,
            });
            current_tvg_name.clear();
            current_group.clear();
        }
    }

    let data = serde_json::json!({
        "groups": sorted_vec(groups),
        "channels": channels
    });
    fs::write(json_path, serde_json::to_string(&data)?)
?;    Ok(())
}

fn sorted_vec(hs: std::collections::HashSet<String>) -> Vec<String> {
    let mut v: Vec<_> = hs.into_iter().collect();
    v.sort();
    v
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let (_, json_path) = get_cache_paths();

    match cli.command {
        Commands::Update => update_data().await?,
        Commands::Groups => {
            if !json_path.exists() { update_data().await?; }
            let data: serde_json::Value = serde_json::from_str(&fs::read_to_string(json_path)?)?;
            println!("⭐ FAVORITES");
            println!("🌐 ALL CHANNELS");
            if let Some(groups) = data["groups"].as_array() {
                for g in groups {
                    println!("{}", g.as_str().unwrap());
                }
            }
        }
        Commands::Channels { group } => {
            let data: serde_json::Value = serde_json::from_str(&fs::read_to_string(json_path)?)?;
            let channels: Vec<Channel> = serde_json::from_value(data["channels"].clone())?;
            for c in channels {
                if group == "🌐 ALL CHANNELS" || c.group == group {
                    let prog = c.epg_now.unwrap_or_else(|| "No Program Info".to_string());
                    println!("[{}] {} | NOW: {} :: {} :: {}", c.group, c.name, prog, c.url, c.name);
                }
            }
        }
        Commands::Play(args) => {
            Command::new("mpv")
                .arg(format!("--title={}", args.title))
                .arg(format!("--force-media-title={}", args.title))
                .arg("--config-dir=/home/admin/Gemini")
                .arg(args.url)
                .spawn()?
                .wait()?;
        }
    }
    Ok(())
}

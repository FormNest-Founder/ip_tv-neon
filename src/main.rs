use std::fs;
use std::path::{Path, PathBuf};
use std::io::{Read, BufReader};
use std::process::Command;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use flate2::read::GzDecoder;
use chrono::{Utc, NaiveDateTime};
use clap::{Parser, Subcommand};
use regex::Regex;
use reqwest::header::USER_AGENT;
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Select};
use console::{style, Term};
use std::os::unix::process::CommandExt;

const RADIO_API: &str = "https://www.radiorecord.ru/api/stations";
const CACHE_DIR: &str = "/tmp/neon_iptv_rs";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Update,
}

#[derive(Clone, Debug)]
struct EpgInfo {
    now_title: String,
    now_stop: i64,
    next_title: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Channel {
    name: String,
    group: String,
    url: String,
    icon: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    playlist_url: String,
    epg_url: String,
}

impl Config {
    fn load() -> Self {
        let path = dirs::config_dir().unwrap().join("neon-iptv/config.json");
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(conf) = serde_json::from_str(&data) { return conf; }
        }
        Self::default()
    }
    fn save(&self) {
        let dir = dirs::config_dir().unwrap().join("neon-iptv");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(dir.join("config.json"), serde_json::to_string(self).unwrap());
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            playlist_url: "http://331273bff393.goodstreem.org/playlists/uplist/bc17084cb401b17401e1001e4c4cb80a/playlist.m3u8".into(),
            epg_url: "http://epg.it999.ru/edem.xml.gz".into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CacheData {
    groups: Vec<String>,
    channels: Vec<Channel>,
}

fn get_cache_paths() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let dir = Path::new(CACHE_DIR);
    let icons = dir.join("icons");
    if !dir.exists() { fs::create_dir_all(dir).ok(); }
    if !icons.exists() { fs::create_dir_all(&icons).ok(); }
    (dir.join("epg.xml"), dir.join("data.json"), dir.join("radio.json"), icons)
}

async fn download_file(url: &str, path: &Path, is_gz: bool) -> Result<()> {
    let client = reqwest::Client::builder().build()?;
    let resp = client.get(url).header(USER_AGENT, "Mozilla/5.0").send().await?.bytes().await?;
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
    let cleaned = re.replace_all(&name, " ");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_epg_time(time_str: &str) -> i64 {
    if time_str.len() < 14 { return 0; }
    if let Ok(dt) = chrono::DateTime::parse_from_str(time_str, "%Y%m%d%H%M%S %z") { return dt.timestamp(); }
    if let Ok(dt) = chrono::DateTime::parse_from_str(time_str, "%Y%m%d%H%M%S%z") { return dt.timestamp(); }
    if let Ok(naive) = NaiveDateTime::parse_from_str(&time_str[0..14], "%Y%m%d%H%M%S") { return naive.and_utc().timestamp(); }
    0
}

fn build_name_to_id_map(path: &Path) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(file) = fs::File::open(path) {
        let mut reader = Reader::from_reader(BufReader::with_capacity(65536, file));
        let mut buf = Vec::new();
        let mut cur_id = String::new();
        let mut in_disp = false;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.name().as_ref() == b"channel" => {
                    cur_id = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"id").map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                },
                Ok(Event::Start(e)) if e.name().as_ref() == b"display-name" => in_disp = true,
                Ok(Event::Text(e)) if in_disp && !cur_id.is_empty() => {
                    map.insert(normalize_name(&String::from_utf8_lossy(e.as_ref())), cur_id.clone());
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"display-name" => in_disp = false,
                Ok(Event::Eof) => break,
                _ => (),
            }
            buf.clear();
        }
    }
    map
}

fn get_category_epg(path: &Path, target_ids: &std::collections::HashSet<String>) -> std::collections::HashMap<String, EpgInfo> {
    let mut results = std::collections::HashMap::new();
    if let Ok(file) = fs::File::open(path) {
        let mut reader = Reader::from_reader(BufReader::with_capacity(65536, file));
        let mut buf = Vec::new();
        let now = Utc::now().timestamp();
        struct TempProg { now: Option<(String, i64)>, next: Option<(String, i64)> }
        let mut temp_map: std::collections::HashMap<String, TempProg> = std::collections::HashMap::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.name().as_ref() == b"programme" => {
                    let id = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"channel").map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                    if target_ids.contains(&id) {
                        let s_str = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"start").map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                        let t_str = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"stop").map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                        let start = parse_epg_time(&s_str);
                        let stop = parse_epg_time(&t_str);
                        let mut title = String::new();
                        let mut title_buf = Vec::new();
                        loop {
                            match reader.read_event_into(&mut title_buf) {
                                Ok(Event::Start(te)) if te.name().as_ref() == b"title" => (),
                                Ok(Event::Text(te)) => title = String::from_utf8_lossy(te.as_ref()).trim().to_string(),
                                Ok(Event::End(te)) if te.name().as_ref() == b"title" => break,
                                Ok(Event::Eof) => break,
                                _ => (),
                            }
                            title_buf.clear();
                        }
                        let entry = temp_map.entry(id).or_insert(TempProg { now: None, next: None });
                        if start <= now && now < stop { entry.now = Some((title, stop)); }
                        else if start >= now && (entry.next.is_none() || start < entry.next.as_ref().unwrap().1) { entry.next = Some((title, start)); }
                    }
                },
                Ok(Event::Eof) => break,
                _ => (),
            }
            buf.clear();
        }
        for (id, tp) in temp_map {
            results.insert(id, EpgInfo {
                now_title: tp.now.as_ref().map(|x| x.0.clone()).unwrap_or_else(|| "...".into()),
                now_stop: tp.now.map(|x| x.1).unwrap_or(0),
                next_title: tp.next.as_ref().map(|x| x.0.clone()).unwrap_or_else(|| "...".into()),
            });
        }
    }
    results
}

async fn update_data(conf: &Config) -> Result<()> {
    let (epg_p, json_p, radio_p, _icons_d) = get_cache_paths();
    println!("📡 {} EPG...", style("Downloading").cyan());
    download_file(&conf.epg_url, &epg_p, true).await?;
    let client = reqwest::Client::builder().user_agent("Mozilla/5.0").build()?;
    let m3u = client.get(&conf.playlist_url).send().await?.text().await?;
    let (mut chans, mut groups, mut name, mut tvg, mut grp, mut logo) = (Vec::new(), std::collections::HashSet::new(), String::new(), String::new(), String::new(), String::new());
    let re_tvg = Regex::new(r#"tvg-name="([^"]+)""#).unwrap();
    let re_logo = Regex::new(r#"tvg-logo="([^"]+)""#).unwrap();
    for line in m3u.lines() {
        if line.starts_with("#EXTINF:") {
            tvg = re_tvg.captures(line).map(|c| c.get(1).unwrap().as_str().to_string()).unwrap_or_default();
            logo = re_logo.captures(line).map(|c| c.get(1).unwrap().as_str().to_string()).unwrap_or_default();
            if let Some(pos) = line.rfind(',') { name = line[pos+1..].trim().to_string(); if tvg.is_empty() { tvg = name.clone(); } }
        } else if let Some(stripped) = line.strip_prefix("#EXTGRP:") { grp = stripped.trim().to_string(); }
        else if line.starts_with("http") {
            if grp.is_empty() { grp = "Other".to_string(); }
            groups.insert(grp.clone());
            chans.push(Channel { name: name.clone(), group: grp.clone(), url: line.trim().to_string(), icon: if logo.is_empty() { None } else { Some(logo.clone()) } });
            tvg.clear(); grp.clear(); logo.clear();
        }
    }
    fs::write(json_p, serde_json::to_string(&CacheData { groups: sorted_vec(groups), channels: chans })?)?;
    let rad: serde_json::Value = client.get(RADIO_API).header(USER_AGENT, UA).send().await?.json().await?;
    let (mut r_stations, mut r_genres) = (Vec::new(), std::collections::HashSet::new());
    if let Some(stations) = rad["result"]["stations"].as_array() {
        for st in stations {
            let (n, u, i_u) = (st["title"].as_str().unwrap_or_default(), st["stream_320"].as_str().unwrap_or_default(), st["icon_fill_colored"].as_str().unwrap_or_default());
            let grp = if let Some(g) = st["genre"].as_array().and_then(|a| a.first()).and_then(|v| v["name"].as_str()) { g.to_string() } else { "Other".to_string() };
            r_genres.insert(grp.clone());
            r_stations.push(Channel { name: n.to_string(), group: grp, url: u.to_string(), icon: Some(i_u.to_string()) });
        }
    }
    fs::write(radio_p, serde_json::to_string(&CacheData { groups: sorted_vec(r_genres), channels: r_stations })?)?;
    println!("✨ {}", style("Update complete!").green().bold());
    Ok(())
}

fn sorted_vec(hs: std::collections::HashSet<String>) -> Vec<String> { let mut v: Vec<_> = hs.into_iter().collect(); v.sort(); v }

async fn run_interactive() -> Result<()> {
    let term = Term::stdout();
    let theme = ColorfulTheme::default();
    let (epg_p, json_p, radio_p, _) = get_cache_paths();
    let mut config = Config::load();

    'source_loop: loop {
        term.clear_screen()?;
        println!("{}", style(" NIGHT CITY NEON HUB ").on_magenta().black().bold());
        let sources = vec!["📺 IPTV", "📻 RADIO", "📂 LOCAL", "🔄 UPDATE", "⏹️ STOP ALL", "⚙️ SETTINGS", "🚪 EXIT"];
        let source_sel = Select::with_theme(&theme).with_prompt("Source").items(&sources).default(0).interact_opt()?;
        match source_sel {
            Some(6) | None => break 'source_loop,
            Some(5) => {
                loop {
                    term.clear_screen()?;
                    println!("{} {}", style(" ⚙️ SETTINGS ").on_yellow().black().bold(), style("(Esc to go back)").dim());
                    let options = vec!["🔗 Edit Playlist URL", "📅 Edit EPG URL", "🔙 BACK"];
                    let sel = Select::with_theme(&theme).items(&options).default(0).interact_opt()?;
                    match sel {
                        Some(0) => {
                            let url: String = dialoguer::Input::with_theme(&theme).with_prompt("New Playlist URL").with_initial_text(&config.playlist_url).interact_text()?;
                            config.playlist_url = url; config.save();
                        },
                        Some(1) => {
                            let url: String = dialoguer::Input::with_theme(&theme).with_prompt("New EPG URL").with_initial_text(&config.epg_url).interact_text()?;
                            config.epg_url = url; config.save();
                        },
                        _ => break,
                    }
                }
                continue;
            },
            Some(4) => { let _ = Command::new("pkill").args(["-9", "-f", "mpv"]).status(); continue; },
            Some(3) => { update_data(&config).await?; continue; },
            _ => (),
        }

        let source_idx = source_sel.unwrap();
        let is_radio = source_idx == 1;
        let data: CacheData = serde_json::from_str(&fs::read_to_string(if is_radio { &radio_p } else { &json_p })?)?;
        let name_to_id = if !is_radio { build_name_to_id_map(&epg_p) } else { std::collections::HashMap::new() };

        'category_loop: loop {
            let mut groups = vec!["🌐 ALL".to_string()];
            groups.extend(data.groups.clone());
            groups.push("🔙 BACK".to_string());
            let cat_sel = Select::with_theme(&theme).with_prompt("Category").items(&groups).default(0).interact_opt()?;
            let group = match cat_sel { Some(idx) if idx < groups.len() - 1 => &groups[idx], _ => break 'category_loop };

            println!("📡 {}", style("Fetching EPG...").cyan());
            let filtered: Vec<&Channel> = data.channels.iter().filter(|c| group == "🌐 ALL" || c.group == *group).collect();
            let mut ids = std::collections::HashSet::new();
            for c in &filtered { if let Some(id) = name_to_id.get(&normalize_name(&c.name)) { ids.insert(id.clone()); } }
            let cat_epg = if !is_radio { get_category_epg(&epg_p, &ids) } else { std::collections::HashMap::new() };

            'channel_loop: loop {
                term.clear_screen()?;
                let now = Utc::now().timestamp();
                let mut names = Vec::new();
                for c in &filtered {
                    let mut epg_str = " | No EPG".to_string();
                    if let Some(id) = name_to_id.get(&normalize_name(&c.name)) {
                        if let Some(info) = cat_epg.get(id) {
                            let left = (info.now_stop - now) / 60;
                            epg_str = format!(" | {} ({}m left) | NEXT: {}", info.now_title, left, info.next_title);
                        }
                    }
                    let name = if c.name.chars().count() > 25 { c.name.chars().take(22).collect::<String>() + "..." } else { c.name.clone() };
                    names.push(format!("{} {:<25}{}", if is_radio { "📻" } else { "📺" }, name, epg_str));
                }
                names.push("🔙 BACK".into());
                let chan_sel = FuzzySelect::new().with_prompt(format!("{} Channels", group)).items(&names).default(0).interact_opt()?;
                match chan_sel {
                    Some(idx) if idx < filtered.len() => {
                        run_mpv(filtered[idx], is_radio);
                        if !is_radio { let _ = term.clear_screen(); std::process::exit(0); }
                    },
                    _ => break 'channel_loop,
                }
            }
        }
    }
    Ok(())
}

fn run_mpv(chan: &Channel, is_radio: bool) {
    let _ = Command::new("pkill").args(["-9", "-f", "mpv"]).status();
    let mut cmd = Command::new("mpv");
    cmd.arg(format!("--user-agent={}", UA)).arg(format!("--title={}", chan.name)).arg("--no-resume-playback").arg("--cache=yes");
    if is_radio { cmd.arg("--no-video").arg("--volume=80").arg("--no-terminal"); }
    else { cmd.arg("--ontop").arg("--fs").arg("--no-terminal"); }
    cmd.arg(&chan.url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).stdin(std::process::Stdio::null());
    unsafe { cmd.pre_exec(|| { libc::setsid(); Ok(()) }); }
    let _ = cmd.spawn();
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load();
    match cli.command {
        Some(Commands::Update) => update_data(&config).await?,
        _ => run_interactive().await?,
    }
    Ok(())
}
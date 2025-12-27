use std::fs;
use std::path::{Path, PathBuf};
use std::io::Read;
use std::process::Command;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use flate2::read::GzDecoder;
use chrono::{Local, Utc, NaiveDateTime};
use clap::{Parser, Subcommand};
use regex::Regex;
use reqwest::header::{USER_AGENT, REFERER, ACCEPT};
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Select};
use console::{style, Term};
use std::os::unix::process::CommandExt;

const PLAYLIST_URL: &str = "http://331273bff393.goodstreem.org/playlists/uplist/bc17084cb401b17401e1001e4c4cb80a/playlist.m3u8";
const EPG_URL: &str = "http://epg.it999.ru/edem.xml.gz";
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

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Channel {
    name: String,
    group: String,
    url: String,
    epg_now: Option<String>,
    icon: Option<String>,
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
    let cleaned = re.replace_all(&name, "");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_epg_time(time_str: &str) -> i64 {
    if time_str.len() < 14 { return 0; }
    
    // С пробелом: "20251227082900 +0300"
    if let Ok(dt) = chrono::DateTime::parse_from_str(time_str, "%Y%m%d%H%M%S %z") {
        return dt.timestamp();
    }
    
    // Без пробела: "20251227082900+0300"
    if let Ok(dt) = chrono::DateTime::parse_from_str(time_str, "%Y%m%d%H%M%S%z") {
        return dt.timestamp();
    }
    
    if let Ok(naive) = NaiveDateTime::parse_from_str(&time_str[0..14], "%Y%m%d%H%M%S") {
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
    let now = Utc::now().timestamp();
    
    // Карта: ID канала -> Список его нормализованных имен
    let mut id_to_names: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut buf = Vec::new();
    let (mut cur_id, mut in_disp, mut in_prog, mut prog_id, mut skip) = (String::new(), false, false, String::new(), false);

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"channel" => {
                    cur_id = e.attributes().filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == b"id")
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                        .unwrap_or_default();
                },
                b"display-name" => in_disp = true,
                b"programme" => {
                    let s = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"start").map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                    let t = e.attributes().filter_map(|a| a.ok()).find(|a| a.key.as_ref() == b"stop").map(|a| String::from_utf8_lossy(&a.value).into_owned()).unwrap_or_default();
                    let start_t = parse_epg_time(&s);
                    let stop_t = parse_epg_time(&t);
                    
                    if start_t <= now && now <= stop_t {
                        prog_id = e.attributes().filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"channel")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                            .unwrap_or_default();
                        skip = false;
                    } else { skip = true; }
                },
                b"title" => if !skip && !prog_id.is_empty() { in_prog = true; },
                _ => (),
            },
            Ok(Event::Text(e)) => {
                let text = String::from_utf8_lossy(e.as_ref()).trim().to_string();
                if text.is_empty() { buf.clear(); continue; }
                
                if in_disp && !cur_id.is_empty() {
                    id_to_names.entry(cur_id.clone()).or_default().push(normalize_name(&text));
                } else if in_prog {
                    if let Some(names) = id_to_names.get(&prog_id) {
                        for name in names {
                            epg_map.insert(name.clone(), text.clone());
                        }
                    }
                }
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"display-name" => in_disp = false,
                b"title" => in_prog = false,
                b"channel" => cur_id.clear(),
                b"programme" => { prog_id.clear(); skip = false; }
                _ => (),
            },
            Ok(Event::Eof) => break,
            _ => (),
        }
        buf.clear();
    }
    epg_map
}

async fn update_data() -> Result<()> {
    let (epg_p, json_p, radio_p, icons_d) = get_cache_paths();
    println!("📡 {} EPG...", style("Downloading").cyan());
    download_file(EPG_URL, &epg_p, true).await?;
    let epg = parse_epg(&epg_p);
    let client = reqwest::Client::builder().user_agent("Mozilla/5.0").build()?;
    let m3u = client.get(PLAYLIST_URL).send().await?.text().await?;
    let (mut chans, mut groups, mut name, mut tvg, mut grp) = (Vec::new(), std::collections::HashSet::new(), String::new(), String::new(), String::new());
    let re_tvg = Regex::new(r#"tvg-name="([^"]+)""#).unwrap();
    for line in m3u.lines() {
        if line.starts_with("#EXTINF:") {
            tvg = re_tvg.captures(line).map(|c| c.get(1).unwrap().as_str().to_string()).unwrap_or_default();
            if let Some(pos) = line.rfind(',') { name = line[pos+1..].trim().to_string(); if tvg.is_empty() { tvg = name.clone(); } }
        } else if line.starts_with("#EXTGRP:") { grp = line[8..].trim().to_string(); }
        else if line.starts_with("http") {
            if grp.is_empty() { grp = "Other".to_string(); }
            groups.insert(grp.clone());
            let prog = epg.get(&normalize_name(&tvg)).or_else(|| epg.get(&normalize_name(&name))).cloned();
            chans.push(Channel { name: name.clone(), group: grp.clone(), url: line.trim().to_string(), epg_now: prog, icon: None });
            tvg.clear(); grp.clear();
        }
    }
    fs::write(json_p, serde_json::to_string(&CacheData { groups: sorted_vec(groups), channels: chans })?)?;
    let rad: serde_json::Value = client.get(RADIO_API).header(REFERER, "https://www.radiorecord.ru/").header(ACCEPT, "application/json").send().await?.json().await?;
    let (mut r_stations, mut r_genres) = (Vec::new(), std::collections::HashSet::new());
    if let Some(stations) = rad["result"]["stations"].as_array() {
        for st in stations {
            let (n, u, i_u) = (st["title"].as_str().unwrap_or_default(), st["stream_320"].as_str().unwrap_or_default(), st["icon_fill_colored"].as_str().unwrap_or_default());
            let i_p = icons_d.join(format!("{}.png", n.replace("/", "_")));
            if !i_p.exists() && !i_u.is_empty() { let _ = download_file(i_u, &i_p, false).await; }
            if let Some(gs) = st["genre"].as_array() {
                for g in gs {
                    let gn = g["name"].as_str().unwrap_or_default().to_string();
                    r_genres.insert(gn.clone());
                    r_stations.push(Channel { name: n.to_string(), group: gn, url: u.to_string(), epg_now: None, icon: Some(i_p.to_string_lossy().to_string()) });
                }
            }
        }
    }
    fs::write(radio_p, serde_json::to_string(&CacheData { groups: sorted_vec(r_genres), channels: r_stations })?)?;
    println!("✨ {}", style("Update complete!").green().bold());
    Ok(())
}

fn sorted_vec(hs: std::collections::HashSet<String>) -> Vec<String> { let mut v: Vec<_> = hs.into_iter().collect(); v.sort(); v }

fn get_local() -> Vec<Channel> {
    let mut c = Vec::new();
    if let Ok(es) = fs::read_dir("/home/admin/Downloads") {
        for e in es.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.extension().map_or(false, |ex| ex == "m3u" || ex == "m3u8") {
                c.push(Channel { name: p.file_name().unwrap().to_string_lossy().to_string(), group: "Downloads".to_string(), url: p.to_string_lossy().to_string(), epg_now: None, icon: None });
            }
        }
    }
    c
}

async fn run_interactive() -> Result<()> {
    let term = Term::stdout();
    let theme = ColorfulTheme::default();
    let (_, json_p, radio_p, _) = get_cache_paths();

    'source_loop: loop {
        term.clear_screen()?;
        println!("{}", style(" NIGHT CITY NEON HUB ").on_magenta().black().bold());
        let sources = vec!["📺 IPTV", "📻 RADIO", "📂 LOCAL (Downloads)", "🔄 UPDATE CACHE", "🚪 EXIT"];
        let source_selection = Select::with_theme(&theme).with_prompt("Select Source (Esc to Exit)").items(&sources).default(0).interact_opt()?;
        
        let source_idx = match source_selection {
            Some(4) | None => break 'source_loop,
            Some(idx) => idx,
        };

        if source_idx == 3 { update_data().await?; continue; }

        let is_radio = source_idx == 1;
        let is_local = source_idx == 2;
        let data = if is_local { CacheData { groups: vec!["Downloads".into()], channels: get_local() } } else {
            let path = if is_radio { &radio_p } else { &json_p };
            if !path.exists() { update_data().await?; }
            serde_json::from_str(&fs::read_to_string(path)?)?
        };

        'category_loop: loop {
            let mut groups = vec!["🌐 ALL".to_string()];
            groups.extend(data.groups.clone());
            groups.push("🔙 BACK TO SOURCES".to_string());
            
            let category_selection = Select::with_theme(&theme).with_prompt("Select Category (Esc for Back)").items(&groups).default(0).interact_opt()?;
            let group_idx = match category_selection { Some(idx) => idx, None => break 'category_loop };
            let group = &groups[group_idx];
            if group == "🔙 BACK TO SOURCES" { break 'category_loop; }

            'channel_loop: loop {
                let filtered: Vec<&Channel> = data.channels.iter().filter(|c| group == "🌐 ALL" || &c.group == group).collect();
                let mut names: Vec<String> = filtered.iter().map(|c| format!("{} | {}", style(&c.name).cyan(), c.epg_now.as_deref().unwrap_or("..."))).collect();
                names.push("🔙 BACK TO CATEGORIES".to_string());
                
                let chan_selection = FuzzySelect::with_theme(&theme).with_prompt(format!("{} > Channel (Esc for Back)", group)).items(&names).default(0).interact_opt()?;
                let chan_idx = match chan_selection { Some(idx) => idx, None => break 'channel_loop };
                if chan_idx == filtered.len() { break 'channel_loop; }
                
                let chan = filtered[chan_idx];
                let _ = Command::new("pkill").args(["-9", "-f", "mpv"]).status();
                
                let mut cmd = Command::new("mpv");
                cmd.arg(format!("--user-agent={}", UA))
                   .arg(format!("--title={}", chan.name))
                   .arg("--no-terminal").arg("--ontop").arg("--fs")
                   .arg("--no-resume-playback").arg("--cache=yes");
                
                if is_radio { cmd.arg("--no-video").arg("--volume=80"); }
                cmd.arg(&chan.url);
                
                cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).stdin(std::process::Stdio::null());

                unsafe {
                    cmd.pre_exec(|| {
                        libc::setsid();
                        Ok(())
                    });
                }

                if let Ok(_) = cmd.spawn() {
                    let _ = term.clear_screen();
                    std::process::exit(0);
                }
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Update) => update_data().await?,
        _ => run_interactive().await?,
    }
    Ok(())
}
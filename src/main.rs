use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use flate2::read::GzDecoder;
use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;
use ratatui::{prelude::*, widgets::*};
use regex::Regex;
use serde::{Deserialize, Serialize};

const CACHE_DIR: &str = "/tmp/neon_iptv_rs";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const RADIO_API: &str = "https://www.radiorecord.ru/api/stations";
const RADIO_NOW_API: &str = "https://www.radiorecord.ru/api/stations/now";
const RECOMMENDED_EPG: &str = "http://epg.one/ru.xml.gz";

static NORM_RE: OnceLock<Regex> = OnceLock::new();
static BRAND_RE: OnceLock<Regex> = OnceLock::new();

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    Update,
    Run,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Config {
    playlist_url: String,
    epg_url: String,
    theme_color: (u8, u8, u8),
    #[serde(default)]
    torrserve_url: String,
    #[serde(default)]
    favorites: HashSet<String>,
    #[serde(default)]
    history: Vec<String>,
    #[serde(default = "default_fullscreen")]
    video_fullscreen: bool,
    #[serde(default = "default_geometry")]
    video_geometry: String,
}
fn default_fullscreen() -> bool {
    true
}
fn default_geometry() -> String {
    "1280x720".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            playlist_url: "http://epg.one/edem_epg_ico2.m3u8".into(),
            epg_url: RECOMMENDED_EPG.into(),
            theme_color: (0, 255, 255),
            torrserve_url: "http://localhost:8090".into(),
            favorites: HashSet::new(),
            history: Vec::new(),
            video_fullscreen: true,
            video_geometry: "1280x720".into(),
        }
    }
}
impl Config {
    fn load() -> Self {
        let p = dirs::config_dir()
            .unwrap_or_else(|| ".".into())
            .join("neon-iptv/config.json");
        let mut cfg: Config = fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if cfg.epg_url.contains("it999.ru") {
            cfg.epg_url = RECOMMENDED_EPG.into();
            let _ = cfg.save();
        }
        if cfg.torrserve_url.is_empty() {
            cfg.torrserve_url = "http://localhost:8090".into();
        }
        cfg
    }
    fn save(&self) -> Result<()> {
        let d = dirs::config_dir()
            .unwrap_or_else(|| ".".into())
            .join("neon-iptv");
        let _ = fs::create_dir_all(&d);
        fs::write(d.join("config.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Channel {
    name: String,
    group: String,
    url: String,
    tvg_id: Option<String>,
    norm_name: String,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct RadioStation {
    id: i64,
    title: String,
    stream: String,
    track: Option<String>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct EpgProgram {
    start: i64,
    stop: i64,
    title: String,
    desc: String,
}
#[derive(Serialize, Deserialize, Default)]
struct AppData {
    channels: Vec<Channel>,
    radio: Vec<RadioStation>,
    groups: Vec<String>,
    epg: HashMap<String, Vec<EpgProgram>>,
    name_to_id: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Debug)]
struct TorrEntry {
    title: String,
    hash: String,
}
#[derive(Deserialize, Clone, Debug)]
struct TorrFile {
    name: String,
    index: i32,
    size: i64,
}

fn normalize(s: &str) -> String {
    let re = NORM_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\(.*\)|HD|FHD|UHD|SD|4K|RU|BY|UA|KAZ|UZB|EST|LAT|LIT|PL|DE|FR|EN|ORIGIN|V\.2|V\.3|\+",
        )
        .unwrap()
    });
    let brand_re = BRAND_RE.get_or_init(|| Regex::new(r"(?i)^(BCU|YOSSO|VF|VIP)\s+").unwrap());
    let stripped = brand_re.replace_all(s, "");
    re.replace_all(&stripped.to_uppercase(), " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_xml_time(s: &str) -> i64 {
    if s.len() >= 20 && let Ok(dt) = DateTime::parse_from_str(&s[..20], "%Y%m%d%H%M%S %z") {
        return dt.timestamp();
    }
    if s.len() >= 14 && let Ok(dt) = NaiveDateTime::parse_from_str(&s[..14], "%Y%m%d%H%M%S") {
        return dt.and_utc().timestamp() - 10800;
    }
    0
}

async fn update_data(config: &Config) -> Result<()> {
    let client = reqwest::Client::builder().user_agent(UA).build()?;
    let mut radio = Vec::new();
    if let Ok(r) = client.get(RADIO_API).send().await
        && let Ok(j) = r.json::<serde_json::Value>().await
        && let Some(st) = j["result"]["stations"].as_array()
    {
        for s in st {
            radio.push(RadioStation {
                id: s["id"].as_i64().unwrap_or(0),
                title: s["title"].as_str().unwrap_or("").into(),
                stream: s["stream_hls"]
                    .as_str()
                    .or(s["stream_320"].as_str())
                    .unwrap_or("")
                    .into(),
                track: None,
            });
        }
    }
    let m3u = if config.playlist_url.starts_with("http") {
        client.get(&config.playlist_url).send().await?.text().await?
    } else {
        std::fs::read_to_string(&config.playlist_url)?
    };
    let mut channels = Vec::new();
    let mut groups = HashSet::new();
    let mut cur_grp = "Other".to_string();
    let re_id = Regex::new(r#"tvg-id="([^"]+)""#).unwrap();
    let re_name = Regex::new(r#"tvg-name="([^"]+)""#).unwrap();
    for line in m3u.lines() {
        if line.starts_with("#EXTINF:") {
            let tid = re_id.captures(line).map(|c| c[1].to_string());
            let tname = re_name.captures(line).map(|c| c[1].to_string());
            let name = line.split(',').next_back().unwrap_or("").trim().to_string();
            let norm = normalize(&name);
            channels.push(Channel {
                name,
                group: cur_grp.clone(),
                url: "".into(),
                tvg_id: tid.or(tname),
                norm_name: norm,
            });
        } else if let Some(g) = line.strip_prefix("#EXTGRP:") {
            cur_grp = g.trim().to_string();
            groups.insert(cur_grp.clone());
        } else if line.starts_with("http") && let Some(ch) = channels.last_mut() {
            ch.url = line.to_string();
        }
    }
    let mut epg: HashMap<String, Vec<EpgProgram>> = HashMap::new();
    let mut name_to_id: HashMap<String, String> = HashMap::new();
    if let Ok(r) = client.get(&config.epg_url).send().await {
        let b = r.bytes().await?;
        let reader_raw: Box<dyn BufRead> = if config.epg_url.ends_with(".gz") {
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
        let mut lang = String::new();
        let now = Utc::now().timestamp();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(XmlEvent::Start(e)) => {
                    tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if tag == "channel" {
                        if let Some(a) = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"id")
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
                                b"stop" => stop = parse_xml_time(&String::from_utf8_lossy(&a.value)),
                                b"channel" => cid = String::from_utf8_lossy(&a.value).into(),
                                _ => {}
                            }
                        }
                        if stop > now - 3600 {
                            cur_prog = Some(EpgProgram {
                                start,
                                stop,
                                title: "".into(),
                                desc: "".into(),
                            });
                            cur_id = cid;
                        }
                    }
                    lang = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"lang")
                        .map(|a| String::from_utf8_lossy(&a.value).to_string())
                        .unwrap_or_default();
                }
                Ok(XmlEvent::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().into_owned();
                    if tag == "display-name"
                        && (lang == "ru"
                            || lang.is_empty()
                            || !name_to_id.contains_key(&normalize(&text)))
                    {
                        name_to_id.insert(normalize(&text), cur_id.clone());
                    }
                    if let Some(p) = cur_prog.as_mut() {
                        if tag == "title" {
                            if lang == "ru" || p.title.is_empty() {
                                p.title = text;
                            }
                        } else if tag == "desc" && (lang == "ru" || p.desc.is_empty()) {
                            p.desc = text;
                        }
                    }
                }
                Ok(XmlEvent::End(e)) => {
                    if e.name().as_ref() == b"programme" && let Some(p) = cur_prog.take() {
                        epg.entry(cur_id.clone()).or_default().push(p);
                    }
                }
                Ok(XmlEvent::Eof) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let mut g_vec: Vec<String> = groups.into_iter().collect();
    g_vec.sort();
    let data = AppData {
        channels,
        radio,
        groups: g_vec,
        epg,
        name_to_id,
    };
    let _ = fs::create_dir_all(CACHE_DIR);
    bincode::serialize_into(
        File::create(Path::new(CACHE_DIR).join("data.bin"))?,
        &data,
    )?;
    Ok(())
}

async fn fetch_radio_now() -> HashMap<i64, String> {
    let client = reqwest::Client::builder().user_agent(UA).build().unwrap();
    let mut map = HashMap::new();
    if let Ok(r) = client.get(RADIO_NOW_API).send().await
        && let Ok(j) = r.json::<serde_json::Value>().await
        && let Some(res) = j["result"].as_array()
    {
        for st in res {
            let id = st["id"].as_i64().unwrap_or(0);
            let artist = st["track"]["artist"].as_str().unwrap_or("");
            let song = st["track"]["song"].as_str().unwrap_or("");
            if !artist.is_empty() {
                map.insert(id, format!("{} - {}", artist, song));
            }
        }
    }
    map
}

fn find_epg_id(ch: &Channel, data: &AppData) -> Option<String> {
    if let Some(id) = &ch.tvg_id
        && data.epg.contains_key(id)
    {
        return Some(id.clone());
    }
    if let Some(id) = data.name_to_id.get(&ch.norm_name) {
        return Some(id.clone());
    }
    if let Some(id) = ch
        .tvg_id
        .as_ref()
        .and_then(|tid| data.name_to_id.get(&normalize(tid)))
    {
        return Some(id.clone());
    }
    None
}

fn get_current_epg<'a>(ch: &Channel, data: &'a AppData, now: i64) -> Option<&'a EpgProgram> {
    let id = find_epg_id(ch, data)?;
    data.epg
        .get(&id)
        .and_then(|progs| progs.iter().find(|p| p.start <= now && p.stop > now))
}

#[derive(PartialEq)]
enum Screen {
    MainMenu,
    CatList,
    ChanList,
    RadioList,
    Detail,
    Settings,
    Input,
    Updating,
    TorrList,
    TorrFiles,
    LinkInput,
}
struct App {
    config: Config,
    data: AppData,
    screen: Screen,
    m_state: ListState,
    cat_state: ListState,
    ch_state: ListState,
    r_state: ListState,
    s_state: ListState,
    t_state: ListState,
    f_state: ListState,
    filtered: Vec<usize>,
    search: String,
    is_search: bool,
    in_buf: String,
    in_tgt: String,
    quit: bool,
    title: String,
    torrents: Vec<TorrEntry>,
    torr_files: Vec<TorrFile>,
    sel_hash: String,
}
impl App {
    fn new(config: Config) -> Self {
        let data = File::open(Path::new(CACHE_DIR).join("data.bin"))
            .ok()
            .and_then(|f| bincode::deserialize_from(f).ok())
            .unwrap_or_default();
        let mut app = Self {
            config,
            data,
            screen: Screen::MainMenu,
            m_state: ListState::default(),
            cat_state: ListState::default(),
            ch_state: ListState::default(),
            r_state: ListState::default(),
            s_state: ListState::default(),
            t_state: ListState::default(),
            f_state: ListState::default(),
            filtered: Vec::new(),
            search: "".into(),
            is_search: false,
            in_buf: "".into(),
            in_tgt: "".into(),
            quit: false,
            title: "".into(),
            torrents: Vec::new(),
            torr_files: Vec::new(),
            sel_hash: "".into(),
        };
        app.m_state.select(Some(0));
        app
    }
    fn stop_all(&mut self) {
        let _ = Command::new("pkill").args(["-9", "-f", "mpv"]).status();
        
    }
    fn run_mpv(&mut self, url: &str, title: &str, sub_title: &str, radio: bool) {
        self.stop_all();
        let display_title = if sub_title.is_empty() {
            title.to_string()
        } else {
            format!("{} │ {}", title, sub_title)
        };
        let is_heavy = title.contains("4K") || title.contains("HDR");
        let mut c = Command::new("mpv");
        c.arg(url)
            .arg("--ontop")
            .arg(format!("--title=NEON: {}", display_title))
            .arg(format!("--force-media-title={}", display_title))
            .arg("--hwdec=auto-safe")
            .arg("--vo=gpu");
        if is_heavy {
            c.arg("--hdr-compute-peak=no")
                .arg("--tone-mapping=bt.2390")
                .arg("--scale=bilinear");
        }
        c.stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null());
        if radio {
            c.arg("--no-video");
            
        } else if self.config.video_fullscreen {
            c.arg("--fs");
        } else {
            c.arg("--no-keepaspect-window")
                .arg(format!("--geometry={}", self.config.video_geometry));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                c.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        let _ = c.spawn();
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let size = f.size();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" NIGHT CITY HUB ")
        .border_style(Style::default().fg(Color::Rgb(
            app.config.theme_color.0,
            app.config.theme_color.1,
            app.config.theme_color.2,
        )));
    f.render_widget(block.clone(), size);
    let area = block.inner(size);
    match app.screen {
        Screen::Updating => {
            f.render_widget(
                Paragraph::new("\n\n🚀 UPDATING DATA...\nPLEASE WAIT...")
                    .alignment(Alignment::Center)
                    .fg(Color::Yellow)
                    .bold(),
                area,
            );
        }
        Screen::MainMenu => {
            let chunks = Layout::default()
                .constraints([Constraint::Length(8), Constraint::Min(0)])
                .split(area);
            f.render_widget(
                Paragraph::new("   NEON HUB\n   V 0.6.3")
                    .alignment(Alignment::Center)
                    .fg(Color::Cyan),
                chunks[0],
            );
            let items = [
                "📺 IPTV",
                "📻 RADIO",
                "📥 TORRSERVE",
                "🔗 PLAY LINK",
                "⭐ FAVORITES",
                "🕒 HISTORY",
                "⏹ STOP ALL",
                "🔄 UPDATE",
                "⚙️ SETTINGS",
                "🚪 EXIT",
            ];
            let list = List::new(items.iter().map(|i| ListItem::new(*i)).collect::<Vec<_>>())
                .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black));
            f.render_stateful_widget(list, chunks[1], &mut app.m_state);
        }
        Screen::CatList => {
            let list = List::new(
                app.data
                    .groups
                    .iter()
                    .map(|g| ListItem::new(format!("📂 {}", g)))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(" Categories "))
            .highlight_style(Style::default().bg(Color::Magenta).fg(Color::Black));
            f.render_stateful_widget(list, area, &mut app.cat_state);
        }
        Screen::ChanList => {
            let chunks = Layout::default()
                .constraints([Constraint::Min(0), Constraint::Length(3)])
                .split(area);
            let now = Utc::now().timestamp();
            let items: Vec<ListItem> = app
                .filtered
                .iter()
                .map(|&idx| {
                    let ch = &app.data.channels[idx];
                    let mut spans = Vec::new();
                    if let Some(cap) = Regex::new(r"^(BCU|BOX|VF|YOSSO|VIP)\s+")
                        .unwrap()
                        .captures(&ch.name)
                    {
                        spans.push(Span::styled(
                            format!("{} ", &cap[1]),
                            Style::default().fg(Color::Cyan).bold(),
                        ));
                        spans.push(Span::styled(
                            &ch.name[cap[0].len()..],
                            Style::default().fg(Color::White),
                        ));
                    } else {
                        spans.push(Span::styled(&ch.name, Style::default().fg(Color::White)));
                    }
                    if ch.name.contains("4K") {
                        spans.push(Span::styled(
                            " [4K]",
                            Style::default().fg(Color::Red).bold(),
                        ));
                    } else if ch.name.contains("HD") || ch.name.contains("FHD") {
                        spans.push(Span::styled(
                            " [HD]",
                            Style::default().fg(Color::Rgb(0, 255, 200)),
                        ));
                    }
                    if let Some(p) = get_current_epg(ch, &app.data, now) {
                        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                        spans.push(Span::styled(
                            format!("NOW: {}", p.title),
                            Style::default().fg(Color::Magenta),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let list = List::new(items)
                .block(Block::default().title(app.title.as_str()).borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::Rgb(30, 30, 50)));
            f.render_stateful_widget(list, chunks[0], &mut app.ch_state);
            f.render_widget(
                Paragraph::new(format!(" SEARCH: {}", app.search)).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
                chunks[1],
            );
        }
        Screen::RadioList => {
            let items: Vec<ListItem> = app
                .data
                .radio
                .iter()
                .map(|r| {
                    let mut spans = vec![Span::styled(
                        format!("󰓇 {} ", r.title),
                        Style::default().fg(Color::Yellow).bold(),
                    )];
                    if let Some(t) = &r.track {
                        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                        spans.push(Span::styled(
                            t.clone(),
                            Style::default().fg(Color::Rgb(255, 0, 255)),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let list = List::new(items)
                .block(Block::default().title(" Radio Record "))
                .highlight_style(Style::default().bg(Color::Rgb(50, 20, 50)));
            f.render_stateful_widget(list, area, &mut app.r_state);
        }
        Screen::TorrList => {
            let items: Vec<ListItem> = app
                .torrents
                .iter()
                .map(|t| ListItem::new(format!("🎬 {}", t.title)))
                .collect();
            let list = List::new(items)
                .block(Block::default().title(" TorrServe Torrents "))
                .highlight_style(Style::default().bg(Color::Rgb(60, 0, 100)));
            f.render_stateful_widget(list, area, &mut app.t_state);
        }
        Screen::TorrFiles => {
            let items: Vec<ListItem> = app
                .torr_files
                .iter()
                .map(|f| {
                    ListItem::new(format!(
                        "📄 {} ({:.2} GB)",
                        f.name,
                        f.size as f64 / 1073741824.0
                    ))
                })
                .collect();
            let list = List::new(items)
                .block(Block::default().title(" Torrent Files "))
                .highlight_style(Style::default().bg(Color::Rgb(20, 20, 50)));
            f.render_stateful_widget(list, area, &mut app.f_state);
        }
        Screen::Detail => {
            let idx = app.filtered[app.ch_state.selected().unwrap_or(0)];
            let ch = &app.data.channels[idx];
            let chunks = Layout::default()
                .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)])
                .split(area);
            f.render_widget(
                Paragraph::new(format!("📺 {}", ch.name))
                    .fg(Color::Cyan)
                    .bold(),
                chunks[0],
            );
            let mut txt = "No description available.".to_string();
            if let Some(id) = find_epg_id(ch, &app.data)
                && let Some(progs) = app.data.epg.get(&id)
            {
                let now = Utc::now().timestamp();
                let mut lines = Vec::new();
                if let Some(p) = progs.iter().find(|p| p.start <= now && p.stop > now) {
                    lines.push(format!("🎬 NOW: {}\n", p.title));
                    if !p.desc.is_empty() {
                        lines.push(format!("📖 {}\n", p.desc));
                    }
                }
                lines.push("\n📅 COMING UP:".into());
                for p in progs.iter().filter(|p| p.start > now).take(5) {
                    let s = DateTime::<Utc>::from_timestamp(p.start, 0)
                        .unwrap()
                        .with_timezone(&Local)
                        .format("%H:%M");
                    lines.push(format!("  {} - {}", s, p.title));
                }
                txt = lines.join("\n");
            }
            f.render_widget(
                Paragraph::new(txt)
                    .block(Block::default().title(" Schedule & Info ").borders(Borders::ALL))
                    .wrap(Wrap { trim: true }),
                chunks[1],
            );
            f.render_widget(
                Paragraph::new("[ENTER] Play  [ESC] Back").alignment(Alignment::Center),
                chunks[2],
            );
        }
        Screen::Settings => {
            let items = [
                format!("Playlist: {}", app.config.playlist_url),
                format!("EPG: {}", app.config.epg_url),
                format!("TorrServe: {}", app.config.torrserve_url),
                format!(
                    "Theme RGB: {},{},{}",
                    app.config.theme_color.0, app.config.theme_color.1, app.config.theme_color.2
                ),
                "Save & Exit".into(),
            ];
            let list = List::new(items.iter().map(|i| ListItem::new(i.as_str())).collect::<Vec<_>>())
                .highlight_style(Style::default().bg(Color::Yellow).fg(Color::Black));
            f.render_stateful_widget(list, area, &mut app.s_state);
        }
        Screen::Input | Screen::LinkInput => {
            let chunks = Layout::default()
                .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(0)])
                .split(area);
            f.render_widget(
                Paragraph::new(format!(" Editing {}:", app.in_tgt)).fg(Color::Cyan),
                chunks[0],
            );
            f.render_widget(
                Paragraph::new(app.in_buf.as_str()).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
                chunks[1],
            );
            f.render_widget(
                Paragraph::new("[ENTER] Save/Run  [ESC] Cancel  [Ctrl+U] Clear  [Paste Enabled]")
                    .fg(Color::DarkGray),
                chunks[2],
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _cli = Cli::parse();
    let config = Config::load();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        event::EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let mut app = App::new(config);
    if !Path::new(CACHE_DIR).join("data.bin").exists() {
        app.screen = Screen::Updating;
        terminal.draw(|f| ui(f, &mut app))?;
        update_data(&app.config).await?;
        app = App::new(Config::load());
    }
    let client = reqwest::Client::builder().user_agent(UA).build()?;
    loop {
        terminal.draw(|f| ui(f, &mut app))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Paste(text) => {
                    if app.screen == Screen::Input || app.screen == Screen::LinkInput {
                        app.in_buf.push_str(&text);
                    }
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match app.screen {
                        Screen::Updating => {} // Do nothing while updating
                        Screen::MainMenu => match key.code {
                            KeyCode::Up => {
                                let i = app.m_state.selected().unwrap_or(0);
                                app.m_state.select(Some(if i == 0 { 9 } else { i - 1 }));
                            }
                            KeyCode::Down => {
                                let i = app.m_state.selected().unwrap_or(0);
                                app.m_state.select(Some(if i == 9 { 0 } else { i + 1 }));
                            },
                            KeyCode::Char('h') => app.screen = Screen::Settings,
                            KeyCode::Char('i') => app.screen = Screen::CatList,
                            KeyCode::Char('r') => {
                                app.screen = Screen::RadioList;
                                // (В идеале тут бы еще обновить треки, но оставим пока так для стабильности)
                            }
                            KeyCode::Enter => match app.m_state.selected().unwrap_or(0) {
                                0 => app.screen = Screen::CatList,
                                1 => {
                                    app.screen = Screen::Updating;
                                    terminal.draw(|f| ui(f, &mut app))?;
                                    let tracks = fetch_radio_now().await;
                                    for r in &mut app.data.radio {
                                        r.track = tracks.get(&r.id).cloned();
                                    }
                                    app.r_state.select(Some(0));
                                    app.screen = Screen::RadioList;
                                }
                                2 => {
                                    app.screen = Screen::Updating;
                                    terminal.draw(|f| ui(f, &mut app))?;
                                    if let Ok(r) = client
                                        .post(format!("{}/torrent", app.config.torrserve_url))
                                        .json(&serde_json::json!({"action":"list"}))
                                        .send()
                                        .await
                                        && let Ok(torrs) = r.json::<Vec<TorrEntry>>().await
                                    {
                                        app.torrents = torrs;
                                        app.t_state.select(Some(0));
                                        app.screen = Screen::TorrList;
                                    } else {
                                        app.screen = Screen::MainMenu;
                                    } // Handle error
                                }
                                3 => {
                                    app.in_buf.clear();
                                    app.in_tgt = "URL/Magnet".into();
                                    app.screen = Screen::LinkInput;
                                }
                                4 => {
                                    app.filtered = app
                                        .data
                                        .channels
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, c)| app.config.favorites.contains(&c.url))
                                        .map(|(i, _)| i)
                                        .collect();
                                    app.title = " Favorites ".into();
                                    app.ch_state.select(Some(0));
                                    app.screen = Screen::ChanList;
                                }
                                5 => {
                                    app.filtered = app
                                        .data
                                        .channels
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, c)| app.config.history.contains(&c.url))
                                        .map(|(i, _)| i)
                                        .collect();
                                    app.title = " History ".into();
                                    app.ch_state.select(Some(0));
                                    app.screen = Screen::ChanList;
                                }
                                6 => app.stop_all(),
                                7 => {
                                    app.screen = Screen::Updating;
                                    terminal.draw(|f| ui(f, &mut app))?;
                                    update_data(&app.config).await?;
                                    app = App::new(Config::load());
                                    terminal.clear()?;
                                } // Re-initialize app after update
                                8 => app.screen = Screen::Settings,
                                9 => app.quit = true,
                                _ => {} // Should not happen
                            },
                            KeyCode::Esc => app.quit = true,
                            _ => {} // Ignore other keys
                        },
                        Screen::CatList => match key.code {
                            KeyCode::Up => {
                                let i = app.cat_state.selected().unwrap_or(0);
                                let l = app.data.groups.len();
                                if l > 0 {
                                    app.cat_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.cat_state.selected().unwrap_or(0);
                                let l = app.data.groups.len();
                                if l > 0 {
                                    app.cat_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(idx) = app.cat_state.selected() {
                                    let g = &app.data.groups[idx];
                                    app.filtered = app
                                        .data
                                        .channels
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, c)| &c.group == g)
                                        .map(|(i, _)| i)
                                        .collect();
                                    app.ch_state.select(Some(0));
                                    app.title = format!(" {} ", g);
                                    app.screen = Screen::ChanList;
                                }
                            }
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            _ => {} // Ignore other keys
                        },
                        Screen::ChanList => match key.code {
                            KeyCode::Up => {
                                let i = app.ch_state.selected().unwrap_or(0);
                                let l = app.filtered.len();
                                if l > 0 {
                                    app.ch_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.ch_state.selected().unwrap_or(0);
                                let l = app.filtered.len();
                                if l > 0 {
                                    app.ch_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Char('/') => {
                                app.is_search = true;
                                app.search.clear();
                            } // Start search
                            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                if let Some(i) = app.ch_state.selected() {
                                    let idx = app.filtered[i];
                                    let u = app.data.channels[idx].url.clone();
                                    if app.config.favorites.contains(&u) {
                                        app.config.favorites.remove(&u);
                                    } else {
                                        app.config.favorites.insert(u);
                                    }
                                    let _ = app.config.save();
                                }
                            } // Toggle favorite
                            KeyCode::Char(c)
                                if !key.modifiers.contains(KeyModifiers::CONTROL)
                                    && !key.modifiers.contains(KeyModifiers::ALT) =>
                            {
                                app.search.push(c);
                                let q = app.search.to_lowercase();
                                app.filtered = app
                                    .data
                                    .channels
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, c)| {
                                        c.norm_name.to_lowercase().contains(&q)
                                            || c.name.to_lowercase().contains(&q)
                                    })
                                    .map(|(i, _)| i)
                                    .collect();
                                app.ch_state.select(Some(0));
                            } // Append char to search
                            KeyCode::Backspace => {
                                app.search.pop();
                                let q = app.search.to_lowercase();
                                app.filtered = app
                                    .data
                                    .channels
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, c)| {
                                        c.norm_name.to_lowercase().contains(&q)
                                            || c.name.to_lowercase().contains(&q)
                                    })
                                    .map(|(i, _)| i)
                                    .collect();
                                app.ch_state.select(Some(0));
                            } // Remove last char from search
                            KeyCode::Enter => {
                                if !app.filtered.is_empty() {
                                    app.screen = Screen::Detail;
                                }
                            } // Enter detail view
                            KeyCode::Esc => {
                                if app.search.is_empty() {
                                    app.screen = Screen::CatList;
                                } else {
                                    app.search.clear();
                                }
                            } // Clear search or go back
                            _ => {} // Ignore other keys
                        },
                        Screen::RadioList => match key.code {
                            KeyCode::Up => {
                                let i = app.r_state.selected().unwrap_or(0);
                                let l = app.data.radio.len();
                                if l > 0 {
                                    app.r_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.r_state.selected().unwrap_or(0);
                                let l = app.data.radio.len();
                                if l > 0 {
                                    app.r_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(i) = app.r_state.selected() {
                                    let (u, t, st) = {
                                        let s = &app.data.radio[i];
                                        (
                                            s.stream.clone(),
                                            s.title.clone(),
                                            s.track.clone().unwrap_or_default(),
                                        )
                                    };
                                    app.run_mpv(&u, &t, &st, true);
                                    app.quit = true;
                                }
                            } // Play radio and quit
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            _ => {} // Ignore other keys
                        },
                        Screen::TorrList => match key.code {
                            KeyCode::Up => {
                                let i = app.t_state.selected().unwrap_or(0);
                                let l = app.torrents.len();
                                if l > 0 {
                                    app.t_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.t_state.selected().unwrap_or(0);
                                let l = app.torrents.len();
                                if l > 0 {
                                    app.t_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(i) = app.t_state.selected() {
                                    let h = app.torrents[i].hash.clone();
                                    app.sel_hash = h.clone();
                                    app.screen = Screen::Updating;
                                    terminal.draw(|f| ui(f, &mut app))?;
                                    if let Ok(r) = client
                                        .post(format!("{}/torrent", app.config.torrserve_url))
                                        .json(&serde_json::json!({"action":"get", "hash":h}))
                                        .send()
                                        .await
                                        && let Ok(j) = r.json::<serde_json::Value>().await
                                    {
                                        if let Some(files) = j["file_list"].as_array() {
                                            app.torr_files = files
                                                .iter()
                                                .map(|f| TorrFile {
                                                    name: f["path"].as_str().unwrap_or("").into(),
                                                    index: f["id"].as_i64().unwrap_or(0) as i32,
                                                    size: f["size"].as_i64().unwrap_or(0),
                                                })
                                                .collect();
                                            app.f_state.select(Some(0));
                                            app.screen = Screen::TorrFiles;
                                        }
                                    } else {
                                        app.screen = Screen::TorrList;
                                    } // Handle error
                                }
                            }
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            _ => {} // Ignore other keys
                        },
                        Screen::TorrFiles => match key.code {
                            KeyCode::Up => {
                                let i = app.f_state.selected().unwrap_or(0);
                                let l = app.torr_files.len();
                                if l > 0 {
                                    app.f_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.f_state.selected().unwrap_or(0);
                                let l = app.torr_files.len();
                                if l > 0 {
                                    app.f_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(i) = app.f_state.selected() {
                                    let (url, name) = {
                                        let f = &app.torr_files[i];
                                        (
                                            format!(
                                                "{}/stream/?link={}&index={}&play",
                                                app.config.torrserve_url, app.sel_hash, f.index
                                            ),
                                            f.name.clone(),
                                        )
                                    };
                                    app.run_mpv(&url, &name, "TorrServe", false);
                                    app.quit = true;
                                }
                            }
                            KeyCode::Esc => app.screen = Screen::TorrList,
                            _ => {} // Ignore other keys
                        },
                        Screen::Detail => match key.code {
                            KeyCode::Enter => {
                                if let Some(i) = app.ch_state.selected() {
                                    let (u, n) = {
                                        let ch = &app.data.channels[app.filtered[i]];
                                        (ch.url.clone(), ch.name.clone())
                                    };
                                    app.run_mpv(&u, &n, "", false);
                                    app.config.history.retain(|x| x != &u);
                                    app.config.history.insert(0, u);
                                    app.config.history.truncate(20);
                                    let _ = app.config.save();
                                }
                            } // Play channel and save to history
                            KeyCode::Esc => app.screen = Screen::ChanList,
                            _ => {} // Ignore other keys
                        },
                        Screen::Settings => match key.code {
                            KeyCode::Up => {
                                let i = app.s_state.selected().unwrap_or(0);
                                app.s_state.select(Some(if i == 0 { 4 } else { i - 1 }));
                            }
                            KeyCode::Down => {
                                let i = app.s_state.selected().unwrap_or(0);
                                app.s_state.select(Some(if i == 4 { 0 } else { i + 1 }));
                            }
                            KeyCode::Enter => match app.s_state.selected().unwrap_or(0) {
                                0 => {
                                    app.in_buf = app.config.playlist_url.clone();
                                    app.in_tgt = "Playlist".into();
                                    app.screen = Screen::Input;
                                }
                                1 => {
                                    app.in_buf = app.config.epg_url.clone();
                                    app.in_tgt = "EPG".into();
                                    app.screen = Screen::Input;
                                }
                                2 => {
                                    app.in_buf = app.config.torrserve_url.clone();
                                    app.in_tgt = "TorrServe".into();
                                    app.screen = Screen::Input;
                                }
                                3 => {
                                    app.in_buf = format!(
                                        "{},{},{}",
                                        app.config.theme_color.0,
                                        app.config.theme_color.1,
                                        app.config.theme_color.2
                                    );
                                    app.in_tgt = "Theme RGB".into();
                                    app.screen = Screen::Input;
                                }
                                4 => {
                                    let _ = app.config.save();
                                    app.screen = Screen::MainMenu;
                                } // Save and exit settings
                                _ => {} // Should not happen
                            },
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            _ => {} // Ignore other keys
                        },
                        Screen::Input => match key.code {
                            KeyCode::Enter => {
                                if app.in_tgt == "Playlist" {
                                    app.config.playlist_url = app.in_buf.clone();
                                } else if app.in_tgt == "EPG" {
                                    app.config.epg_url = app.in_buf.clone();
                                } else if app.in_tgt == "Theme RGB" {
                                    let c: Vec<u8> = app
                                        .in_buf
                                        .split(",")
                                        .map(|s| s.trim().parse().unwrap_or(0))
                                        .collect();
                                    if c.len() == 3 {
                                        app.config.theme_color = (c[0], c[1], c[2]);
                                    }
                                } else {
                                    app.config.torrserve_url = app.in_buf.clone();
                                }
                                app.screen = Screen::Settings;
                            } // Save input and go back to settings
                            KeyCode::Esc => app.screen = Screen::Settings,
                            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.in_buf.clear()
                            } // Clear input buffer
                            KeyCode::Char(c) => app.in_buf.push(c), // Append character to input buffer
                            KeyCode::Backspace => {
                                app.in_buf.pop();
                            } // Remove last character from input buffer
                            _ => {} // Ignore other keys
                        },
                        Screen::LinkInput => match key.code {
                            KeyCode::Enter => {
                                if !app.in_buf.is_empty() {
                                    let u = app.in_buf.clone();
                                    app.run_mpv(&u, "Link", "", false);
                                    app.quit = true;
                                }
                            } // Play link and quit
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.in_buf.clear()
                            } // Clear input buffer
                            KeyCode::Char(c) => app.in_buf.push(c), // Append character to input buffer
                            KeyCode::Backspace => {
                                app.in_buf.pop();
                            } // Remove last character from input buffer
                            _ => {} // Ignore other keys
                        },
                    }
                    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        app.quit = true;
                    } // Ctrl+C to quit
                }
                _ => {} // Ignore other events
            }
        }
        if app.quit {
            break;
        } // Exit loop if quit flag is set
    }
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        event::DisableBracketedPaste
    )?;
    terminal.show_cursor()?;
    Ok(())
}

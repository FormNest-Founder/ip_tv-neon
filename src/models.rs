use crate::utils::RECOMMENDED_EPG;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub playlist_url: String,
    pub epg_url: String,
    pub theme_color: (u8, u8, u8),
    #[serde(default)]
    pub favorites: HashSet<String>,
    #[serde(default)]
    pub history: Vec<String>,
    #[serde(default = "default_fullscreen")]
    pub video_fullscreen: bool,
    #[serde(default = "default_geometry")]
    pub video_geometry: String,
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
            favorites: HashSet::new(),
            history: Vec::new(),
            video_fullscreen: true,
            video_geometry: "1280x720".into(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let d = dirs::config_dir()
            .unwrap_or_else(|| ".".into())
            .join("neon-iptv");
        let p = d.join("config.json");
        let cfg: Config = fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        cfg
    }
    pub fn save(&self) -> Result<()> {
        let d = dirs::config_dir()
            .unwrap_or_else(|| ".".into())
            .join("neon-iptv");
        let _ = fs::create_dir_all(&d);
        fs::write(d.join("config.json"), serde_json::to_string_pretty(self)?).unwrap();
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Channel {
    pub name: String,
    pub group: String,
    pub url: String,
    pub tvg_id: Option<String>,
    pub norm_name: String,
    #[serde(default)]
    pub catchup_days: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RadioStation {
    pub id: String,
    pub title: String,
    pub stream: String,
    pub genres: Vec<String>,
    pub provider: String,
    pub track: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EpgProgram {
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub desc: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct AppData {
    pub channels: Vec<Channel>,
    pub radio: Vec<RadioStation>,
    pub radio_groups: Vec<String>,
    pub groups: Vec<String>,
    pub epg: HashMap<String, Vec<EpgProgram>>,
    pub name_to_id: HashMap<String, String>,
}

#[derive(PartialEq, Debug)]
pub enum Screen {
    MainMenu,
    CatList,
    ChanList,
    RadioCatList,
    RadioList,
    Detail,
    Settings,
    Input,
    Updating,
    LocalList,
    LinkInput,
}

use crate::consts::*;
use crate::utils::main_log;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-local counter guaranteeing each temp file name is unique even when
/// two saves land within the same wall-clock tick (CG1).
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub playlist_url: String,
    pub epg_url: String,
    pub theme_color: (u8, u8, u8),
    #[serde(default)]
    pub favorites: HashSet<String>,
    #[serde(default)]
    pub history: Vec<String>,
    /// URL → channel name cache (для отображения без загруженного плейлиста)
    #[serde(default)]
    pub channel_names: HashMap<String, String>,
    #[serde(default = "default_fullscreen")]
    pub video_fullscreen: bool,
    #[serde(default = "default_geometry")]
    pub video_geometry: String,
    #[serde(default)]
    pub local_dir: String,
    /// Selected AI model: a `MODEL_CATALOG` id token (e.g. "deepseek",
    /// "gemini", "agy:claude-opus-4-6"). Resolved via `ai::resolve_choice`,
    /// which migrates legacy values and never panics. Stored as `String` so the
    /// serde_json config schema is unchanged (no cache/bincode bump needed).
    #[serde(default)]
    pub llm_provider: String,
}

fn default_fullscreen() -> bool {
    true
}
fn default_geometry() -> String {
    "1280x720".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            playlist_url: String::new(),
            epg_url: RECOMMENDED_EPG.into(),
            theme_color: (0, 255, 255),
            favorites: HashSet::new(),
            history: Vec::new(),
            channel_names: HashMap::new(),
            video_fullscreen: true,
            video_geometry: "1280x720".into(),
            local_dir: String::new(),
            llm_provider: String::new(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        Self::load_from(&get_config_json_path())
    }

    /// Load config from an explicit path. On a parse error the corrupt file is
    /// renamed to `<name>.bak` (logged loudly) BEFORE falling back to Default, so
    /// the next save() cannot silently overwrite the user's favorites/history.
    pub fn load_from(path: &std::path::Path) -> Self {
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return Self::default(), // no file yet — first run
        };
        match serde_json::from_str(&raw) {
            Ok(cfg) => cfg,
            Err(e) => {
                let bak = path.with_extension("json.bak");
                match fs::rename(path, &bak) {
                    Ok(()) => main_log(&format!(
                        "[config] parse error ({e}) — corrupt config backed up to {}",
                        bak.display()
                    )),
                    Err(re) => main_log(&format!(
                        "[config] parse error ({e}) — FAILED to back up corrupt config: {re}"
                    )),
                }
                Self::default()
            }
        }
    }

    /// Durable save (fsync before rename) — use for favorites/settings changes.
    pub fn save(&mut self) -> Result<()> {
        self.save_inner(true)
    }

    /// Non-durable save (skips fsync) — used for high-frequency history-only
    /// updates where a crash losing the very last watched entry is acceptable and
    /// an fsync per channel change is not worth the I/O stall.
    pub fn save_fast(&mut self) -> Result<()> {
        self.save_inner(false)
    }

    fn save_inner(&mut self, durable: bool) -> Result<()> {
        // O(n) prune: build a set of history URLs once instead of scanning the
        // history Vec for every channel_names entry (was O(entries × history)).
        let history_set: HashSet<&String> = self.history.iter().collect();
        self.channel_names
            .retain(|url, _| history_set.contains(url) || self.favorites.contains(url));
        use std::io::Write;
        let dir = get_config_dir();
        fs::create_dir_all(&dir)?;
        let path = get_config_json_path();
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), seq));
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
            if durable {
                f.sync_all()?;
            }
        }
        fs::rename(&tmp, &path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn history_push(&mut self, url: &str, name: &str) {
        self.history.retain(|u| u != url);
        self.history.push(url.to_string());
        if self.history.len() > 200 {
            self.history.drain(0..self.history.len() - 200);
        }
        if !name.is_empty() {
            self.channel_names.insert(url.to_string(), name.to_string());
        }
        // History is high-frequency and low-value on crash — skip fsync.
        if let Err(e) = self.save_fast() {
            main_log(&format!("[config] save failed: {e}"));
        }
    }

    pub fn favorite_add(&mut self, url: &str, name: &str) {
        self.favorites.insert(url.to_string());
        if !name.is_empty() {
            self.channel_names.insert(url.to_string(), name.to_string());
        }
        if let Err(e) = self.save() {
            main_log(&format!("[config] save failed: {e}"));
        }
    }

    pub fn favorite_remove(&mut self, url: &str) {
        self.favorites.remove(url);
        // Не удаляем из channel_names — может пригодиться для истории
        if let Err(e) = self.save() {
            main_log(&format!("[config] save failed: {e}"));
        }
    }

    /// Получить имя канала: сначала из кеша, потом fallback на URL
    pub fn channel_name<'a>(&'a self, url: &'a str) -> &'a str {
        self.channel_names
            .get(url)
            .map(|s| s.as_str())
            .unwrap_or(url)
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
    #[serde(default)]
    pub name_lower: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RadioStation {
    pub id: String,
    pub title: String,
    pub stream: String,
    #[serde(default)]
    pub quality_urls: HashMap<String, String>,
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
    #[serde(default)]
    pub group_counts: HashMap<String, usize>,
    /// url → channel index, for O(1) reverse lookups. Derived state, never
    /// persisted (rebuilt on load via `build_url_index`), so the on-disk cache
    /// format is unchanged.
    #[serde(skip)]
    pub url_index: HashMap<String, usize>,
    /// radio genre (uppercased) → station count, for the genre list. Derived,
    /// not persisted.
    #[serde(skip)]
    pub radio_group_counts: HashMap<String, usize>,
}

impl AppData {
    /// Rebuild the derived reverse-lookup indices after (de)serialization.
    pub fn build_indices(&mut self) {
        self.url_index = self
            .channels
            .iter()
            .enumerate()
            .map(|(i, ch)| (ch.url.clone(), i))
            .collect();
        let mut radio_counts: HashMap<String, usize> = HashMap::new();
        for st in &self.radio {
            for g in &st.genres {
                *radio_counts.entry(g.clone()).or_insert(0) += 1;
            }
        }
        self.radio_group_counts = radio_counts;
    }
}

#[derive(Serialize, Deserialize)]
pub struct CacheContainer {
    pub version: u32,
    pub data: AppData,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Screen {
    MainMenu,
    CatList,
    ChanList,
    Detail,
    RadioCatList,
    RadioList,
    Settings,
    SettingsEdit(usize),
    Updating,
    LocalList,
    #[allow(dead_code)]
    LinkInput,
    Favorites,
    History,
    AiChat,
}

pub const SETTINGS_COUNT: usize = 9;
pub const SETTINGS_LABELS: [&str; SETTINGS_COUNT] = [
    "Playlist URL",
    "EPG URL",
    "Fullscreen",
    "Window Geometry",
    "Theme",
    "AI Provider",
    "Local Playlists Dir",
    "Clear History",
    "Clear Favorites",
];

pub const THEME_PRESETS: &[(u8, u8, u8, &str)] = &[
    (0, 255, 255, "Cyan"),
    (255, 0, 255, "Magenta"),
    (0, 255, 128, "Neon Green"),
    (255, 128, 0, "Orange"),
    (128, 0, 255, "Purple"),
    (255, 255, 0, "Yellow"),
    (255, 0, 0, "Red"),
];

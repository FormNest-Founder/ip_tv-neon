// ─── Imports ─────────────────────────────────────────────────────────────────

use crate::ai::{AiSearchResult, ChatMsg};
use crate::epg::{find_epg_id, load_data};
use crate::models::{AppData, Config, EpgProgram, Screen};
use crate::utils::main_log;
use ratatui::widgets::ListState;
use std::path::PathBuf;

// ─── Detail Screen Sub-State ────────────────────────────────────────────────

#[derive(Default)]
pub struct DetailState {
    pub channel: Option<usize>,
    pub programs: Vec<EpgProgram>,
    pub current_idx: Option<usize>,
    pub return_screen: Option<Screen>,
}

// ─── AI Chat Sub-State ───────────────────────────────────────────────────────

#[derive(Default)]
pub struct AiState {
    pub query: String,
    pub results: Vec<AiSearchResult>,
    pub loading: bool,
    pub chat_history: Vec<ChatMsg>,
    pub focus_results: bool,
    pub chat_scroll: u16,
}

// ─── Navigation Sub-State ────────────────────────────────────────────────────

#[derive(Default)]
pub struct NavState {
    pub m_state: ListState,
    pub cat_state: ListState,
    pub ch_state: ListState,
    pub r_state: ListState,
    pub r_cat_state: ListState,
    pub d_state: ListState,
    pub fav_state: ListState,
    pub hist_state: ListState,
    pub set_state: ListState,
    pub epg_state: ListState,
    pub ai_state: ListState,
    pub filtered: Vec<usize>,
    pub filtered_radio: Vec<usize>,
    pub selected_group: String,
    pub selected_radio_genre: String,
    pub search: String,
    pub edit_buf: String,
    #[expect(dead_code)]
    pub quality_popup: Option<usize>,
}

// ─── App State ───────────────────────────────────────────────────────────────

pub struct App {
    pub config: Config,
    pub data: AppData,
    pub screen: Screen,
    pub nav: NavState,
    pub quit: bool,
    pub suspended: bool,
    pub local_files: Vec<PathBuf>,
    pub player: crate::player::PlayerController,
    pub needs_redraw: bool,
    pub debug: bool,
    pub status_msg: Option<String>,
    pub detail: DetailState,
    pub ai: AiState,
    /// Whether the agy CLI binary was found at startup (drives the Settings hint).
    pub agy_available: bool,
}

/// Accept only http/https media URLs (CG4). Case-insensitive on the scheme.
pub fn is_http_url(url: &str) -> bool {
    let u = url.trim_start();
    let lower = u.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Build a flussonic catch-up (archive) URL.
///
/// `utc` is the requested archive start (program start time). `lutc` must be
/// the *current* live-edge timestamp (i.e. "now"), NOT the program's stop time:
/// flussonic interprets `lutc` as the current wall-clock of the live stream and
/// uses it to anchor the catch-up window. Passing the program stop made the
/// provider serve the live edge / an empty window instead of the past archive.
fn build_archive_url(base: &str, utc: i64, lutc: i64) -> String {
    format!("{base}?utc={utc}&lutc={lutc}")
}

// ─── Constructor & Data Loading ──────────────────────────────────────────────

impl App {
    pub fn new(config: Config) -> Self {
        let data = load_data();

        let mut app = Self {
            config,
            data,
            screen: Screen::MainMenu,
            nav: NavState::default(),
            quit: false,
            suspended: false,
            local_files: Vec::new(),
            player: crate::player::PlayerController::new(),
            needs_redraw: true,
            debug: false,
            status_msg: None,
            detail: DetailState::default(),
            ai: AiState::default(),
            agy_available: crate::ai::agy_available(),
        };
        app.nav.m_state.select(Some(0));
        app.nav.cat_state.select(Some(0));
        app.nav.r_cat_state.select(Some(0));
        app.nav.set_state.select(Some(0));
        app.backfill_channel_names();
        app
    }

    pub fn reload_data(&mut self) {
        self.data = load_data();
        self.backfill_channel_names();
    }

    /// Заполнить channel_names из загруженного плейлиста для favorites/history
    fn backfill_channel_names(&mut self) {
        let mut dirty = false;
        let urls: Vec<String> = self
            .config
            .favorites
            .iter()
            .chain(self.config.history.iter())
            .cloned()
            .collect();
        for url in &urls {
            if self.config.channel_names.contains_key(url) {
                continue;
            }
            if let Some(ch) = self.data.channels.iter().find(|ch| ch.url == *url) {
                self.config
                    .channel_names
                    .insert(url.clone(), ch.name.clone());
                dirty = true;
            }
        }
        if dirty {
            if let Err(e) = self.config.save() {
                main_log(&format!("[config] save failed: {e}"));
            }
        }
    }

    pub fn stop_all(&mut self) {
        self.player.stop_all();
        self.suspended = false;
    }

    // ─── Radio VU + Marquee Tick (called every 50ms) ──────────────────────

    /// Advance VU-meter simulation and marquee scroll by one 50ms tick.
    /// Should only be called while radio_ipc.is_some().
    pub fn tick_radio(&mut self) {
        self.player.tick_radio();
    }

    // ─── Detail / EPG Playback ────────────────────────────────────────────

    pub fn open_detail(&mut self, channel_idx: usize) {
        if channel_idx >= self.data.channels.len() {
            return;
        }
        self.detail.return_screen = Some(self.screen);
        let ch = &self.data.channels[channel_idx];
        self.detail.channel = Some(channel_idx);

        let now = chrono::Utc::now().timestamp();
        let epg_id = find_epg_id(ch, &self.data);
        let programs: Vec<EpgProgram> = if let Some(id) = epg_id {
            if let Some(progs) = self.data.epg.get(&id) {
                let cutoff = if ch.catchup_days > 0 {
                    now - (ch.catchup_days as i64 * 86400)
                } else {
                    now - 3600
                };
                progs.iter().filter(|p| p.stop > cutoff).cloned().collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let current_idx = programs.iter().position(|p| now >= p.start && now < p.stop);
        self.detail.programs = programs;
        self.detail.current_idx = current_idx;

        let select = current_idx.unwrap_or(0);
        self.nav
            .epg_state
            .select(if self.detail.programs.is_empty() {
                None
            } else {
                Some(select)
            });
        self.screen = Screen::Detail;
    }

    pub fn detail_play_selected(&mut self) {
        let ch_idx = match self.detail.channel {
            Some(i) if i < self.data.channels.len() => i,
            _ => return,
        };
        let ch = &self.data.channels[ch_idx];
        let url = ch.url.clone();
        let name = ch.name.clone();

        let epg_idx = self.nav.epg_state.selected();
        let now = chrono::Utc::now().timestamp();

        if let Some(idx) = epg_idx {
            if idx < self.detail.programs.len() {
                let prog_title = self.detail.programs[idx].title.clone();
                let prog_start = self.detail.programs[idx].start;
                let prog_stop = self.detail.programs[idx].stop;
                let is_current = now >= prog_start && now < prog_stop;
                let is_future = prog_start > now;

                if is_current || is_future {
                    self.run_mpv(&url, &name, &prog_title, false);
                } else if ch.catchup_days > 0 {
                    // lutc = now (live edge), not prog_stop — flussonic catch-up convention.
                    let archive_url = build_archive_url(&url, prog_start, now);
                    self.run_mpv(&archive_url, &name, &prog_title, false);
                } else {
                    self.run_mpv(&url, &name, &prog_title, false);
                }
                self.config.history_push(&url, &name);
                return;
            }
        }

        self.run_mpv(&url, &name, "", false);
        self.config.history_push(&url, &name);
    }

    pub fn detail_play_live(&mut self) {
        let ch_idx = match self.detail.channel {
            Some(i) if i < self.data.channels.len() => i,
            _ => return,
        };
        let ch = &self.data.channels[ch_idx];
        let url = ch.url.clone();
        let name = ch.name.clone();
        self.run_mpv(&url, &name, "", false);
        self.config.history_push(&url, &name);
    }

    // ─── MPV Player ──────────────────────────────────────────────────────

    pub fn run_mpv(&mut self, url: &str, title: &str, sub_title: &str, radio: bool) {
        self.player.run_mpv(url, title, sub_title, radio, &self.config, self.debug, &mut self.status_msg);
    }

    // ─── AI Playback ────────────────────────────────────────────────────

    pub fn ai_play_selected(&mut self) {
        let idx = match self.nav.ai_state.selected() {
            Some(i) if i < self.ai.results.len() => i,
            _ => return,
        };
        let result = self.ai.results[idx].clone();
        if result.channel_idx >= self.data.channels.len() {
            return;
        }
        let ch = &self.data.channels[result.channel_idx];
        let url = ch.url.clone();
        let name = ch.name.clone();
        let now = chrono::Utc::now().timestamp();

        if result.program.start > 0 {
            let is_current = now >= result.program.start && now < result.program.stop;
            let is_future = result.program.start > now;
            if is_current || is_future {
                self.run_mpv(&url, &name, &result.program.title, false);
            } else if ch.catchup_days > 0 {
                // lutc = now (live edge), not program.stop — flussonic catch-up convention.
                let archive_url = build_archive_url(&url, result.program.start, now);
                self.run_mpv(&archive_url, &name, &result.program.title, false);
            } else {
                self.run_mpv(&url, &name, &result.program.title, false);
            }
        } else {
            self.run_mpv(&url, &name, "", false);
        }
        self.config.history_push(&url, &name);
    }

    // ─── Favorites & Filtering ────────────────────────────────────────────

    pub fn sorted_favorites(&self) -> Vec<&String> {
        let mut favs: Vec<_> = self.config.favorites.iter().collect();
        favs.sort_by(|a, b| {
            let name_a = self.config.channel_name(a);
            let name_b = self.config.channel_name(b);
            name_a.cmp(name_b)
        });
        favs
    }

    pub fn update_filter(&mut self) {
        let q = self.nav.search.to_lowercase();
        let group = &self.nav.selected_group;
        self.nav.filtered = self
            .data
            .channels
            .iter()
            .enumerate()
            .filter(|(_, ch)| ch.group == *group)
            .filter(|(_, ch)| q.is_empty() || ch.name_lower.contains(&q))
            .map(|(i, _)| i)
            .collect();
        if self.nav.filtered.is_empty() && !q.is_empty() {
            self.nav.filtered = self
                .data
                .channels
                .iter()
                .enumerate()
                .filter(|(_, ch)| ch.name_lower.contains(&q))
                .map(|(i, _)| i)
                .collect();
        }
        self.nav.ch_state.select(if self.nav.filtered.is_empty() {
            None
        } else {
            Some(0)
        });
        self.needs_redraw = true;
    }

    pub fn update_radio_filter(&mut self) {
        let genre = &self.nav.selected_radio_genre;
        self.nav.filtered_radio = self
            .data
            .radio
            .iter()
            .enumerate()
            .filter(|(_, r)| genre == "All" || r.genres.contains(genre))
            .map(|(i, _)| i)
            .collect();
        self.nav
            .r_state
            .select(if self.nav.filtered_radio.is_empty() {
                None
            } else {
                Some(0)
            });
        self.needs_redraw = true;
    }

    // ─── Settings ────────────────────────────────────────────────────────

    pub fn settings_value(&self, idx: usize) -> String {
        use crate::models::THEME_PRESETS;
        match idx {
            0 => self.config.playlist_url.clone(),
            1 => self.config.epg_url.clone(),
            2 => {
                if self.config.video_fullscreen {
                    "ON".into()
                } else {
                    "OFF".into()
                }
            }
            3 => self.config.video_geometry.clone(),
            4 => {
                let c = self.config.theme_color;
                THEME_PRESETS
                    .iter()
                    .find(|p| (p.0, p.1, p.2) == c)
                    .map(|p| p.3.to_string())
                    .unwrap_or_else(|| format!("({},{},{})", c.0, c.1, c.2))
            }
            5 => {
                let mut label = crate::ai::choice_label(&self.config.llm_provider).to_string();
                let backend = crate::ai::resolve_choice(&self.config.llm_provider).backend;
                if backend == crate::ai::Backend::Agy && !self.agy_available {
                    label.push_str(" (agy not found)");
                }
                label
            }
            6 => {
                if self.config.local_dir.is_empty() {
                    "~/  ~/Downloads  ~/Videos".into()
                } else {
                    self.config.local_dir.clone()
                }
            }
            7 => format!("{} entries", self.config.history.len()),
            8 => format!("{} entries", self.config.favorites.len()),
            _ => String::new(),
        }
    }

    pub fn settings_apply(&mut self, idx: usize, val: &str) {
        match idx {
            0 => self.config.playlist_url = val.to_string(),
            1 => self.config.epg_url = val.to_string(),
            3 => self.config.video_geometry = val.to_string(),
            6 => self.config.local_dir = val.to_string(),
            _ => {}
        }
        if let Err(e) = self.config.save() {
            self.status_msg = Some(format!("Save failed: {e}"));
        }
    }

    pub fn settings_toggle(&mut self, idx: usize) {
        use crate::models::THEME_PRESETS;
        match idx {
            2 => {
                self.config.video_fullscreen = !self.config.video_fullscreen;
                if let Err(e) = self.config.save() {
                    main_log(&format!("[config] save failed: {e}"));
                }
            }
            4 => {
                let cur = self.config.theme_color;
                let pos = THEME_PRESETS
                    .iter()
                    .position(|p| (p.0, p.1, p.2) == cur)
                    .unwrap_or(0);
                let next = (pos + 1) % THEME_PRESETS.len();
                let p = THEME_PRESETS[next];
                self.config.theme_color = (p.0, p.1, p.2);
                if let Err(e) = self.config.save() {
                    main_log(&format!("[config] save failed: {e}"));
                }
            }
            5 => {
                self.config.llm_provider =
                    crate::ai::next_choice_id(&self.config.llm_provider).to_string();
                if let Err(e) = self.config.save() {
                    main_log(&format!("[config] save failed: {e}"));
                }
            }
            7 => {
                self.config.history.clear();
                if let Err(e) = self.config.save() {
                    main_log(&format!("[config] save failed: {e}"));
                }
                self.status_msg = Some("History cleared".into());
            }
            8 => {
                self.config.favorites.clear();
                if let Err(e) = self.config.save() {
                    main_log(&format!("[config] save failed: {e}"));
                }
                self.status_msg = Some("Favorites cleared".into());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_archive_url;

    #[test]
    fn archive_url_uses_now_as_lutc_not_prog_stop() {
        let base = "http://host/iptv/TOKEN/204/index.m3u8";
        let prog_start = 1_700_000_000_i64;
        let prog_stop = prog_start + 1800; // a past program end
        let now = 1_700_100_000_i64; // current live edge, later than prog_stop

        let url = build_archive_url(base, prog_start, now);

        // utc must stay the program start (the requested archive window).
        assert!(url.contains(&format!("utc={prog_start}")));
        // lutc must be "now", never the program stop — that was the bug.
        assert!(url.ends_with(&format!("lutc={now}")));
        assert!(!url.contains(&format!("lutc={prog_stop}")));
    }
}

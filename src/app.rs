use crate::consts::*;
use crate::epg::find_epg_id;
use crate::models::{AppData, CacheContainer, Config, EpgProgram, Screen};
use crate::utils::main_log;
use ratatui::widgets::ListState;
use std::fs::File;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};

pub struct App {
    pub config: Config,
    pub data: AppData,
    pub screen: Screen,
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
    pub filtered: Vec<usize>,
    pub filtered_radio: Vec<usize>,
    pub selected_group: String,
    pub selected_radio_genre: String,
    pub search: String,
    pub edit_buf: String,
    pub quit: bool,
    pub suspended: bool,
    pub local_files: Vec<PathBuf>,
    pub mpv_handle: Option<Child>,
    pub needs_redraw: bool,
    pub debug: bool,
    pub status_msg: Option<String>,
    pub detail_channel: Option<usize>,
    pub detail_programs: Vec<EpgProgram>,
    pub detail_current_idx: Option<usize>,
}

impl App {
    pub fn new(config: Config) -> Self {
        let path = get_data_bin_path();
        let data = if let Ok(f) = File::open(&path) {
            match bincode::deserialize_from::<_, CacheContainer>(f) {
                Ok(c) if c.version == APP_VERSION => c.data,
                _ => {
                    let _ = std::fs::remove_file(path);
                    AppData::default()
                }
            }
        } else {
            AppData::default()
        };

        let mut app = Self {
            config,
            data,
            screen: Screen::MainMenu,
            m_state: ListState::default(),
            cat_state: ListState::default(),
            ch_state: ListState::default(),
            r_state: ListState::default(),
            r_cat_state: ListState::default(),
            d_state: ListState::default(),
            fav_state: ListState::default(),
            hist_state: ListState::default(),
            set_state: ListState::default(),
            epg_state: ListState::default(),
            filtered: Vec::new(),
            filtered_radio: Vec::new(),
            selected_group: String::new(),
            selected_radio_genre: String::new(),
            search: String::new(),
            edit_buf: String::new(),
            quit: false,
            suspended: false,
            local_files: Vec::new(),
            mpv_handle: None,
            needs_redraw: true,
            debug: false,
            status_msg: None,
            detail_channel: None,
            detail_programs: Vec::new(),
            detail_current_idx: None,
        };
        app.m_state.select(Some(0));
        app.cat_state.select(Some(0));
        app.r_cat_state.select(Some(0));
        app.set_state.select(Some(0));
        app
    }

    pub fn reload_data(&mut self) {
        let path = get_data_bin_path();
        if let Ok(f) = File::open(&path) {
            if let Ok(c) = bincode::deserialize_from::<_, CacheContainer>(f) {
                if c.version == APP_VERSION {
                    self.data = c.data;
                }
            }
        }
    }

    pub fn stop_all(&mut self) {
        if let Some(mut child) = self.mpv_handle.take() {
            let _ = child.start_kill();
        }
        self.suspended = false;
    }

    pub fn open_detail(&mut self, channel_idx: usize) {
        let ch = &self.data.channels[channel_idx];
        self.detail_channel = Some(channel_idx);

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
        self.detail_programs = programs;
        self.detail_current_idx = current_idx;

        let select = current_idx.unwrap_or(0);
        self.epg_state.select(if self.detail_programs.is_empty() { None } else { Some(select) });
        self.screen = Screen::Detail;
    }

    pub fn detail_play_selected(&mut self) {
        let ch_idx = match self.detail_channel {
            Some(i) => i,
            None => return,
        };
        let ch = &self.data.channels[ch_idx];
        let url = ch.url.clone();
        let name = ch.name.clone();

        let epg_idx = self.epg_state.selected();
        let now = chrono::Utc::now().timestamp();

        if let Some(idx) = epg_idx {
            if idx < self.detail_programs.len() {
                let prog_title = self.detail_programs[idx].title.clone();
                let prog_start = self.detail_programs[idx].start;
                let prog_stop = self.detail_programs[idx].stop;
                let is_current = now >= prog_start && now < prog_stop;
                let is_future = prog_start > now;

                if is_current || is_future {
                    self.run_mpv(&url, &name, &prog_title, false);
                } else if ch.catchup_days > 0 {
                    let archive_url = format!("{}?utc={}&lutc={}", url, prog_start, prog_stop);
                    self.run_mpv(&archive_url, &name, &prog_title, false);
                } else {
                    self.run_mpv(&url, &name, &prog_title, false);
                }
                self.config.history_push(&url);
                return;
            }
        }

        self.run_mpv(&url, &name, "", false);
        self.config.history_push(&url);
    }

    pub fn detail_play_live(&mut self) {
        let ch_idx = match self.detail_channel {
            Some(i) => i,
            None => return,
        };
        let ch = &self.data.channels[ch_idx];
        let url = ch.url.clone();
        let name = ch.name.clone();
        self.run_mpv(&url, &name, "", false);
        self.config.history_push(&url);
    }

    pub fn run_mpv(&mut self, url: &str, title: &str, sub_title: &str, radio: bool) {
        self.stop_all();

        let display_title = if sub_title.is_empty() {
            title.to_string()
        } else {
            format!("{} | {}", title, sub_title)
        };

        let mut c = Command::new("mpv");
        c.arg(format!("--title=NEON: {}", display_title))
            .arg(format!("--force-media-title={}", display_title))
            .arg("--volume=20")
            .arg("--ontop")
            .arg("--no-ytdl");

        if self.debug {
            c.arg("--log-file=/tmp/neon_mpv.log");
            main_log(&format!("MPV launch: url={} radio={} title={}", url, radio, display_title));
        }

        if radio {
            c.arg("--no-video");
            c.arg(url);
            c.stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null());
            #[cfg(unix)]
            unsafe { c.pre_exec(|| { libc::setsid(); Ok(()) }); }
            match c.spawn() {
                Ok(child) => { self.mpv_handle = Some(child); }
                Err(e) => { self.status_msg = Some(format!("MPV error: {}", e)); }
            }
        } else {
            if self.config.video_fullscreen {
                c.arg("--fs");
            } else {
                c.arg("--no-keepaspect-window").arg(format!("--geometry={}", self.config.video_geometry));
            }
            c.arg(url);
            c.stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null());
            #[cfg(unix)]
            unsafe { c.pre_exec(|| { libc::setsid(); Ok(()) }); }
            match c.spawn() {
                Ok(child) => {
                    self.mpv_handle = Some(child);
                    self.suspended = true;
                    // Hide TUI window via niri
                    let _ = std::process::Command::new("niri").args(["msg", "action", "move-column-to-workspace", "--focus", "false", "4"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
                }
                Err(e) => { self.status_msg = Some(format!("MPV error: {}", e)); }
            }
        }
    }

    pub fn sorted_favorites(&self) -> Vec<&String> {
        let mut favs: Vec<_> = self.config.favorites.iter().collect();
        favs.sort();
        favs
    }

    pub fn update_filter(&mut self) {
        let q = self.search.to_lowercase();
        let group = &self.selected_group;
        self.filtered = self.data.channels.iter().enumerate()
            .filter(|(_, ch)| ch.group == *group)
            .filter(|(_, ch)| q.is_empty() || ch.name_lower.contains(&q))
            .map(|(i, _)| i).collect();
        if self.filtered.is_empty() && !q.is_empty() {
            self.filtered = self.data.channels.iter().enumerate()
                .filter(|(_, ch)| ch.name_lower.contains(&q))
                .map(|(i, _)| i).collect();
        }
        self.ch_state.select(if self.filtered.is_empty() { None } else { Some(0) });
        self.needs_redraw = true;
    }

    pub fn update_radio_filter(&mut self) {
        let genre = &self.selected_radio_genre;
        self.filtered_radio = self.data.radio.iter().enumerate()
            .filter(|(_, r)| genre == "All" || r.genres.contains(genre))
            .map(|(i, _)| i).collect();
        self.r_state.select(if self.filtered_radio.is_empty() { None } else { Some(0) });
        self.needs_redraw = true;
    }

    pub fn settings_value(&self, idx: usize) -> String {
        use crate::models::THEME_PRESETS;
        match idx {
            0 => self.config.playlist_url.clone(),
            1 => self.config.epg_url.clone(),
            2 => if self.config.video_fullscreen { "ON".into() } else { "OFF".into() },
            3 => self.config.video_geometry.clone(),
            4 => {
                let c = self.config.theme_color;
                THEME_PRESETS.iter().find(|p| (p.0, p.1, p.2) == c)
                    .map(|p| p.3.to_string())
                    .unwrap_or_else(|| format!("({},{},{})", c.0, c.1, c.2))
            }
            5 => format!("{} entries", self.config.history.len()),
            6 => format!("{} entries", self.config.favorites.len()),
            _ => String::new(),
        }
    }

    pub fn settings_apply(&mut self, idx: usize, val: &str) {
        match idx {
            0 => self.config.playlist_url = val.to_string(),
            1 => self.config.epg_url = val.to_string(),
            3 => self.config.video_geometry = val.to_string(),
            _ => {}
        }
        let _ = self.config.save();
    }

    pub fn settings_toggle(&mut self, idx: usize) {
        use crate::models::THEME_PRESETS;
        match idx {
            2 => { self.config.video_fullscreen = !self.config.video_fullscreen; let _ = self.config.save(); }
            4 => {
                let cur = self.config.theme_color;
                let pos = THEME_PRESETS.iter().position(|p| (p.0, p.1, p.2) == cur).unwrap_or(0);
                let next = (pos + 1) % THEME_PRESETS.len();
                let p = THEME_PRESETS[next];
                self.config.theme_color = (p.0, p.1, p.2);
                let _ = self.config.save();
            }
            5 => { self.config.history.clear(); let _ = self.config.save(); self.status_msg = Some("History cleared".into()); }
            6 => { self.config.favorites.clear(); let _ = self.config.save(); self.status_msg = Some("Favorites cleared".into()); }
            _ => {}
        }
    }
}

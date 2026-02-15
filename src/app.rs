use crate::models::{AppData, CacheContainer, Config, Screen};
use crate::consts::*;
use ratatui::widgets::ListState;
use regex::Regex;
use std::fs::File;
use std::sync::LazyLock;
use std::path::PathBuf;
use std::process::{Command, Stdio, Child};

static CLEAN_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(BCU|BOX|VF|YOSSO|VIP)\s+").unwrap());

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
    pub filtered: Vec<usize>,
    pub filtered_radio: Vec<usize>,
    pub selected_group: String,
    pub selected_radio_genre: String,
    pub search: String,
    pub quit: bool,
    pub local_files: Vec<PathBuf>,
    pub last_error: Option<String>,
    pub mpv_handle: Option<Child>,
    pub needs_redraw: bool,
}

impl App {
    pub fn reload_data(&mut self) {
        let path = get_data_bin_path();
        if let Ok(f) = File::open(&path) {
            if let Ok(container) = bincode::deserialize_from::<_, CacheContainer>(f) {
                if container.version == APP_VERSION {
                    self.data = container.data;
                }
            }
        }
    }

    pub fn new(config: Config) -> Self {
        let path = get_data_bin_path();
        let data = if let Ok(f) = File::open(&path) {
            match bincode::deserialize_from::<_, CacheContainer>(f) {
                Ok(container) if container.version == APP_VERSION => container.data,
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
            filtered: Vec::new(),
            filtered_radio: Vec::new(),
            selected_group: String::new(),
            selected_radio_genre: String::new(),
            search: String::new(),
            quit: false,
            local_files: Vec::new(),
            last_error: None,
            mpv_handle: None,
            needs_redraw: true,
        };
        app.m_state.select(Some(0));
        app.cat_state.select(Some(0));
        app.r_cat_state.select(Some(0));
        app
    }

    pub fn stop_all(&mut self) {
        if let Some(mut child) = self.mpv_handle.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

        pub fn run_mpv(&mut self, url: &str, title: &str, sub_title: &str, radio: bool) {
        let _ = Command::new("pkill").arg("-f").arg("NEON").status();
        self.stop_all();
        let display_title = if sub_title.is_empty() { title.to_string() } else { format!("{} │ {}", title, sub_title) };
        let mut c = Command::new("mpv");
        c.arg(format!("--title=NEON: {}", display_title))
            .arg(format!("--force-media-title={}", display_title))
            .arg("--volume=20")
            .arg("--ontop")
            
                                    .arg("--no-ytdl");

        c.stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null());

        if radio {
            c.arg("--no-video");
        } else {
            if self.config.video_fullscreen {
                c.arg("--fs");
            } else {
                c.arg("--no-keepaspect-window").arg(format!("--geometry={}", self.config.video_geometry));
            }
        }

        c.arg(url);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe { c.pre_exec(|| { libc::setsid(); Ok(()) }); }
        }
        if let Ok(child) = c.spawn() { self.mpv_handle = Some(child); }
    }
    pub fn clean_name(&self, name: &str) -> String {
        CLEAN_REGEX.replace(name, "").to_string()
    }

    pub fn update_filter(&mut self) {
        let q = self.search.to_lowercase();
        let group = &self.selected_group;
        self.filtered = self.data.channels.iter().enumerate()
            .filter(|(_, ch)| ch.group == *group)
            .filter(|(_, ch)| q.is_empty() || ch.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.ch_state.select(if self.filtered.is_empty() { None } else { Some(0) });
        self.needs_redraw = true;
    }

    pub fn update_radio_filter(&mut self) {
        let genre = &self.selected_radio_genre;
        self.filtered_radio = self.data.radio.iter().enumerate()
            .filter(|(_, r)| genre == "All" || r.genres.contains(genre))
            .map(|(i, _)| i)
            .collect();
        self.r_state.select(if self.filtered_radio.is_empty() { None } else { Some(0) });
        self.needs_redraw = true;
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // mpv survives exit
    }
}

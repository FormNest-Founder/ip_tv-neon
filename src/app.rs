use crate::models::{AppData, Config, Screen};
use crate::utils::get_cache_dir;
use ratatui::widgets::ListState;
use regex::Regex;
use std::fs::File;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct App {
    pub config: Config,
    pub data: AppData,
    pub screen: Screen,
    pub m_state: ListState,
    pub cat_state: ListState,
    pub ch_state: ListState,
    pub r_state: ListState,
    pub s_state: ListState,
    pub l_state: ListState,
    pub r_cat_state: ListState,
    pub d_state: ListState,
    pub filtered: Vec<usize>,
    pub search: String,
    pub _is_search: bool,
    pub in_buf: String,
    pub in_tgt: String,
    pub quit: bool,
    pub title: String,
    pub local_files: Vec<PathBuf>,
    pub last_error: Option<String>,
    pub clean_regex: Regex,
}

impl App {
    pub fn new(config: Config) -> Self {
        let cache_dir = get_cache_dir();
        let data = File::open(cache_dir.join("data.bin"))
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
            l_state: ListState::default(),
            r_cat_state: ListState::default(),
            d_state: ListState::default(),
            filtered: Vec::new(),
            search: "".into(),
            _is_search: false,
            in_buf: "".into(),
            in_tgt: "".into(),
            quit: false,
            title: "".into(),
            local_files: Vec::new(),
            last_error: None,
            clean_regex: Regex::new(r"^(BCU|BOX|VF|YOSSO|VIP)\s+").unwrap(),
        };
        app.m_state.select(Some(0));
        app
    }
    pub fn stop_all(&mut self) {
        let _ = Command::new("pkill").args(["-9", "-f", "mpv"]).status();
    }
    pub fn run_mpv(&mut self, url: &str, title: &str, sub_title: &str, radio: bool) {
        self.stop_all();
        let display_title = if sub_title.is_empty() {
            title.to_string()
        } else {
            format!("{} │ {}", title, sub_title)
        };
        let is_heavy = title.contains("4K") || title.contains("HDR");
        let mut c = Command::new("mpv");
        c.arg(format!("--title=NEON: {}", display_title))
            .arg(format!("--force-media-title={}", display_title))
            .arg("--volume=15")
            .arg("--ontop")
            .arg("--hwdec=vaapi-copy")
            .arg("--vo=gpu-next")
            .arg("--no-ytdl");

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

        c.arg(url);

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

// ─── Imports ─────────────────────────────────────────────────────────────────

use crate::ai::{AiSearchResult, ChatMsg};
use crate::epg::{find_epg_id, load_data};
use crate::models::{AppData, Config, EpgProgram, Screen};
use crate::mpv_ipc::{spawn_ipc_task, IpcHandle, RadioState, SharedRadioState};
use crate::utils::main_log;
use ratatui::widgets::ListState;
use std::fs::File;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::process::{Child, Command};



// ─── VU Constants ────────────────────────────────────────────────────────────

pub const VU_BARS: usize = 20;
/// How long a peak indicator stays at max before falling (in ticks at 50ms)
const PEAK_HOLD_TICKS: u32 = 10;

// ─── Radio Visuals Sub-State ─────────────────────────────────────────────────

pub struct RadioVisuals {
    pub vu_bars: [f32; VU_BARS],
    pub vu_peaks: [f32; VU_BARS],
    pub vu_peak_hold: [u32; VU_BARS],
    pub radio_start: Option<std::time::Instant>,
    pub marquee_offset: usize,
    pub marquee_tick: u32,
}

impl Default for RadioVisuals {
    fn default() -> Self {
        Self {
            vu_bars: [0.0; VU_BARS],
            vu_peaks: [0.0; VU_BARS],
            vu_peak_hold: [0; VU_BARS],
            radio_start: None,
            marquee_offset: 0,
            marquee_tick: 0,
        }
    }
}

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
    pub mpv_handle: Option<Child>,
    pub radio_ipc: Option<IpcHandle>,
    pub radio_state: SharedRadioState,
    pub radio_station_title: String,
    pub visuals: RadioVisuals,
    pub needs_redraw: bool,
    pub debug: bool,
    pub status_msg: Option<String>,
    pub detail: DetailState,
    pub ai: AiState,
    /// Whether the agy CLI binary was found at startup (drives the Settings hint).
    pub agy_available: bool,
}

/// Accept only http/https media URLs (CG4). Case-insensitive on the scheme.
fn is_http_url(url: &str) -> bool {
    let u = url.trim_start();
    let lower = u.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
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
            mpv_handle: None,
            radio_ipc: None,
            radio_state: Arc::new(Mutex::new(RadioState::default())),
            radio_station_title: String::new(),
            visuals: RadioVisuals::default(),
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
        // Stop via IPC if available (graceful), otherwise kill
        if let Some(ref ipc) = self.radio_ipc {
            ipc.quit();
        }
        self.radio_ipc = None;

        if let Some(mut child) = self.mpv_handle.take() {
            let _ = child.start_kill();
        }
        self.suspended = false;
        *self
            .radio_state
            .lock()
            .expect("radio_state poisoned in stop_all") = RadioState::default();
        self.radio_station_title.clear();
        self.visuals.radio_start = None;
        self.visuals.vu_bars = [0.0; VU_BARS];
        self.visuals.vu_peaks = [0.0; VU_BARS];
        self.visuals.vu_peak_hold = [0; VU_BARS];
        self.visuals.marquee_offset = 0;
        self.visuals.marquee_tick = 0;
    }

    // ─── Radio VU + Marquee Tick (called every 50ms) ──────────────────────

    /// Advance VU-meter simulation and marquee scroll by one 50ms tick.
    /// Should only be called while radio_ipc.is_some().
    pub fn tick_radio(&mut self) {
        use rand::Rng;

        let (paused, muted, volume) = {
            let st = self
                .radio_state
                .lock()
                .expect("radio_state poisoned in tick_radio");
            (st.paused, st.muted, st.volume)
        };

        let elapsed_s = self
            .visuals
            .radio_start
            .map(|s| s.elapsed().as_secs_f32())
            .unwrap_or(0.0);

        // Volume/state scale factor for VU amplitude
        let amp_scale = if paused || muted {
            0.0f32
        } else {
            (volume as f32 / 100.0).clamp(0.0, 1.0)
        };

        // Frequency weights per bar: bass on the left, highs on the right
        // Shape: raised cosine bump centred at each frequency band
        let mut rng = rand::thread_rng();
        for i in 0..VU_BARS {
            let pos = i as f32 / (VU_BARS - 1) as f32; // 0..1

            // Bass weight peaks at pos=0, mid at 0.4, high at 1.0
            let w_bass = (1.0 - pos).powi(2);
            let w_mid = 1.0 - (pos - 0.4).abs() * 2.5;
            let w_mid = w_mid.clamp(0.0, 1.0);
            let w_high = pos.powi(2);

            let t = elapsed_s;
            let bass = (t * 2.0 + i as f32 * 0.3).sin().abs();
            let mid = (t * 5.0 + i as f32 * 0.7).sin().abs() * 0.7;
            let hi = (t * 11.0 + i as f32 * 1.3).sin().abs() * 0.5;
            let noise: f32 = rng.gen::<f32>() * 0.2;

            let target = (bass * w_bass + mid * w_mid + hi * w_high + noise).min(1.0) * amp_scale;

            // Low-pass: fast rise, slow decay
            let alpha = if target > self.visuals.vu_bars[i] {
                0.6
            } else {
                0.25
            };
            self.visuals.vu_bars[i] = self.visuals.vu_bars[i] * (1.0 - alpha) + target * alpha;

            // Peak: hold then fall slowly
            if self.visuals.vu_bars[i] >= self.visuals.vu_peaks[i] {
                self.visuals.vu_peaks[i] = self.visuals.vu_bars[i];
                self.visuals.vu_peak_hold[i] = PEAK_HOLD_TICKS;
            } else if self.visuals.vu_peak_hold[i] > 0 {
                self.visuals.vu_peak_hold[i] -= 1;
            } else {
                self.visuals.vu_peaks[i] =
                    (self.visuals.vu_peaks[i] - 0.03).max(self.visuals.vu_bars[i]);
            }
        }

        // Marquee: advance 1 char every 5 ticks (≈250ms)
        self.visuals.marquee_tick += 1;
        if self.visuals.marquee_tick >= 5 {
            self.visuals.marquee_tick = 0;
            self.visuals.marquee_offset += 1;
        }
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
                    let archive_url = format!("{}?utc={}&lutc={}", url, prog_start, prog_stop);
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
        // Protocol whitelist (CG4): only http/https reach mpv. Blocks file://,
        // edl://, and other local/pseudo protocols a crafted playlist could
        // inject. Checked BEFORE stop_all so a bad URL never kills playback.
        if !is_http_url(url) {
            let scheme = url.split(':').next().unwrap_or("?");
            main_log(&format!("[security] blocked non-http media URL scheme: {scheme}"));
            self.status_msg = Some("Blocked: only http/https media URLs are allowed".into());
            return;
        }
        self.stop_all();

        let display_title = if sub_title.is_empty() {
            title.to_string()
        } else {
            format!("{} | {}", title, sub_title)
        };

        let mut c = Command::new("mpv");
        c.arg(format!("--force-media-title={}", display_title))
            .arg("--volume=20")
            .arg("--no-ytdl");

        if self.debug {
            let mpv_log = std::env::temp_dir().join("neon_mpv.log");
            c.arg(format!("--log-file={}", mpv_log.display()));
            let safe_url = url.split('?').next().unwrap_or("(url)");
            main_log(&format!(
                "MPV launch: url={} radio={} title={}",
                safe_url, radio, display_title
            ));
        }

        if radio {
            let sock = std::env::temp_dir()
                .join(format!("neon-mpv-{}.sock", std::process::id()))
                .to_string_lossy()
                .to_string();
            c.arg("--no-video")
                .arg("--vo=null")
                .arg("--force-window=no")
                .arg("--idle=yes")
                .arg("--keep-open=yes")
                .arg(format!("--input-ipc-server={}", sock));
            c.arg("--").arg(url);
            c.stdout(Stdio::null()).stdin(Stdio::null());
            // Redirect stderr to log so failures are visible (not silent)
            let err_log = std::env::temp_dir().join("neon_mpv_stderr.log");
            match File::create(err_log) {
                Ok(f) => {
                    c.stderr(Stdio::from(f));
                }
                Err(_) => {
                    c.stderr(Stdio::null());
                }
            }
            #[cfg(unix)]
            unsafe {
                c.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            match c.spawn() {
                Ok(child) => {
                    self.mpv_handle = Some(child);
                    let state = Arc::clone(&self.radio_state);
                    let handle = spawn_ipc_task(sock, state);
                    self.radio_ipc = Some(handle);
                    self.radio_station_title = title.to_string();
                    self.visuals.radio_start = Some(Instant::now());
                }
                Err(e) => {
                    self.status_msg = Some(format!("MPV error: {}", e));
                }
            }
        } else {
            c.arg(format!("--title=NEON: {}", display_title))
                .arg("--ontop")
                .arg("--force-window=immediate")
                // HLS Optimizations
                .arg("--cache=yes")
                .arg("--demuxer-max-bytes=1000MiB")
                .arg("--demuxer-max-back-bytes=500MiB")
                .arg("--hls-bitrate=max") 
                .arg("--demuxer-lavf-o=http_persistent=1") 
                .arg("--network-timeout=10")
                // FFmpeg & GPU Optimizations (AMD Vega 11)
                .arg("--hwdec=auto-safe") // Enable FFmpeg VAAPI hardware decoding
                .arg("--vo=gpu-next") // Modern GPU renderer
                .arg("--gpu-api=vulkan") // Use Vulkan (RADV) instead of OpenGL
                .arg("--vd-lavc-threads=4"); // Multi-threading for fallback CPU decoding

            if self.config.video_fullscreen {
                c.arg("--fs");
            } else {
                c.arg("--no-keepaspect-window")
                    .arg(format!("--geometry={}", self.config.video_geometry));
            }
            c.arg("--").arg(url);
            c.stdout(Stdio::null())
                .stderr(Stdio::null())
                .stdin(Stdio::null());
            #[cfg(unix)]
            unsafe {
                c.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            match c.spawn() {
                Ok(child) => {
                    self.mpv_handle = Some(child);
                }
                Err(e) => {
                    self.status_msg = Some(format!("MPV error: {}", e));
                }
            }
        }
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
                let archive_url = format!(
                    "{}?utc={}&lutc={}",
                    url, result.program.start, result.program.stop
                );
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

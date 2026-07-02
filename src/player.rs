use crate::mpv_ipc::{spawn_ipc_task, IpcHandle, RadioState, SharedRadioState};
use crate::utils::main_log;
use crate::models::Config;
use std::fs::File;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::process::{Child, Command};

pub const VU_BARS: usize = 20;
const PEAK_HOLD_TICKS: u32 = 10;

/// Build a private, hard-to-guess path for the mpv IPC control socket.
///
/// Prefer `$XDG_RUNTIME_DIR` (a per-user 0700 tmpfs) so the control socket is not
/// world-accessible; fall back to the app's own user-private cache dir, never the
/// shared world-writable `/tmp`. A random-ish suffix (nanos + pid) avoids
/// collisions and makes the path unpredictable to other local users.
fn radio_socket_path() -> String {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(crate::consts::get_cache_dir);
    let _ = std::fs::create_dir_all(&base);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    base.join(format!("neon-mpv-{}-{:x}.sock", std::process::id(), nanos))
        .to_string_lossy()
        .into_owned()
}

pub struct RadioVisuals {
    pub vu_bars: [f32; VU_BARS],
    pub vu_peaks: [f32; VU_BARS],
    pub vu_peak_hold: [u32; VU_BARS],
    pub radio_start: Option<std::time::Instant>,
    pub marquee_offset: usize,
    pub marquee_tick: u32,
    /// State for the tiny xorshift PRNG driving cosmetic VU noise (replaces the
    /// `rand` crate — this noise has no cryptographic or statistical need).
    rng_state: u32,
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
            rng_state: 0x9E37_79B9, // any non-zero seed
        }
    }
}

/// xorshift32 → f32 in [0, 1). Cosmetic only.
fn xorshift_unit(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

pub struct PlayerController {
    pub mpv_handle: Option<Child>,
    pub radio_ipc: Option<IpcHandle>,
    pub radio_state: SharedRadioState,
    pub radio_station_title: String,
    pub visuals: RadioVisuals,
}

impl Default for PlayerController {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerController {
    pub fn new() -> Self {
        Self {
            mpv_handle: None,
            radio_ipc: None,
            radio_state: Arc::new(Mutex::new(RadioState::default())),
            radio_station_title: String::new(),
            visuals: RadioVisuals::default(),
        }
    }

    pub fn stop_all(&mut self) {
        if let Some(ref ipc) = self.radio_ipc {
            ipc.quit();
            ipc.abort(); // stop the reader task so it can't reconnect to the next mpv on the reused socket
        }
        self.radio_ipc = None;

        if let Some(mut child) = self.mpv_handle.take() {
            let _ = child.start_kill();
        }
        *self
            .radio_state
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = RadioState::default();
        self.radio_station_title.clear();
        self.visuals.radio_start = None;
        self.visuals.vu_bars = [0.0; VU_BARS];
        self.visuals.vu_peaks = [0.0; VU_BARS];
        self.visuals.vu_peak_hold = [0; VU_BARS];
        self.visuals.marquee_offset = 0;
        self.visuals.marquee_tick = 0;
    }

    pub fn tick_radio(&mut self) {
        let (paused, muted, volume) = {
            let st = self
                .radio_state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            (st.paused, st.muted, st.volume)
        };

        let elapsed_s = self
            .visuals
            .radio_start
            .map(|s| s.elapsed().as_secs_f32())
            .unwrap_or(0.0);

        let amp_scale = if paused || muted {
            0.0f32
        } else {
            (volume as f32 / 100.0).clamp(0.0, 1.0)
        };

        for i in 0..VU_BARS {
            let pos = i as f32 / (VU_BARS - 1) as f32;

            let w_bass = (1.0 - pos).powi(2);
            let w_mid = 1.0 - (pos - 0.4).abs() * 2.5;
            let w_mid = w_mid.clamp(0.0, 1.0);
            let w_high = pos.powi(2);

            let t = elapsed_s;
            let bass = (t * 2.0 + i as f32 * 0.3).sin().abs();
            let mid = (t * 5.0 + i as f32 * 0.7).sin().abs() * 0.7;
            let hi = (t * 11.0 + i as f32 * 1.3).sin().abs() * 0.5;
            let noise: f32 = xorshift_unit(&mut self.visuals.rng_state) * 0.2;

            let target = (bass * w_bass + mid * w_mid + hi * w_high + noise).min(1.0) * amp_scale;

            let alpha = if target > self.visuals.vu_bars[i] {
                0.6
            } else {
                0.25
            };
            self.visuals.vu_bars[i] = self.visuals.vu_bars[i] * (1.0 - alpha) + target * alpha;

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

        self.visuals.marquee_tick += 1;
        if self.visuals.marquee_tick >= 5 {
            self.visuals.marquee_tick = 0;
            self.visuals.marquee_offset += 1;
        }
    }

    pub fn run_mpv(
        &mut self,
        url: &str,
        title: &str,
        sub_title: &str,
        radio: bool,
        config: &Config,
        debug: bool,
        status_msg: &mut Option<String>,
    ) {
        if !crate::app::is_http_url(url) {
            let scheme = url.split(':').next().unwrap_or("?");
            main_log(&format!("[security] blocked non-http media URL scheme: {scheme}"));
            *status_msg = Some("Blocked: only http/https media URLs are allowed".into());
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

        if debug {
            let mpv_log = std::env::temp_dir().join("neon_mpv.log");
            c.arg(format!("--log-file={}", mpv_log.display()));
            let safe_url = url.split('?').next().unwrap_or("(url)");
            main_log(&format!(
                "MPV launch: url={} radio={} title={}",
                safe_url, radio, display_title
            ));
        }

        if radio {
            let sock = radio_socket_path();
            c.arg("--no-video")
                .arg("--vo=null")
                .arg("--force-window=no")
                .arg("--idle=yes")
                .arg("--keep-open=yes")
                .arg(format!("--input-ipc-server={}", sock));
            c.arg("--").arg(url);
            c.stdout(Stdio::null()).stdin(Stdio::null());
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
                    *status_msg = Some(format!("MPV error: {}", e));
                }
            }
        } else {
            c.arg(format!("--title=NEON: {}", display_title))
                .arg("--ontop")
                .arg("--force-window=immediate")
                .arg("--cache=yes")
                .arg("--demuxer-max-bytes=1000MiB")
                .arg("--demuxer-max-back-bytes=500MiB")
                .arg("--hls-bitrate=max")
                .arg("--demuxer-lavf-o=http_persistent=1")
                .arg("--network-timeout=10")
                // No --hwdec / --gpu-api on the CLI: both are platform-specific and live in
                // ~/.config/mpv/mpv.conf (hwdec=vaapi-copy + gpu-api=vulkan on Vega 11 / RADV gfx902 —
                // bare vaapi or hwdec=auto pick a vulkan decoder that crashes HEVC; gpu-api=vulkan also
                // pins the backend so mpv never probes EGL, which disconnects this APU's display). A CLI
                // value would OVERRIDE mpv.conf (the original Vega 11 crash) — deferring to it is the
                // real hardware decoupling, and a machine without an mpv.conf falls back to mpv's safe
                // software decode + default backend, not a crash. --vo=gpu-next is platform-agnostic, kept.
                .arg("--vo=gpu-next")
                .arg("--vd-lavc-threads=4");

            if config.video_fullscreen {
                c.arg("--fs");
            } else {
                c.arg("--no-keepaspect-window")
                    .arg(format!("--geometry={}", config.video_geometry));
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
                    *status_msg = Some(format!("MPV error: {}", e));
                }
            }
        }
    }
}

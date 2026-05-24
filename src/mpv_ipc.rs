// ─── MPV IPC Client ───────────────────────────────────────────────────────────
//
// Communicates with mpv via JSON IPC over a Unix socket.
// mpv opens the socket slightly after launch, so connection is retried up to 2s.

use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::{sleep, timeout, Duration, Instant};

// ─── Shared State ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RadioState {
    pub media_title: String,
    /// Raw icy-title string, usually "Artist - Track"
    pub icy_title: String,
    /// Station name from icy-name metadata tag
    pub icy_name: String,
    /// Artist extracted from metadata (artist or icy-title prefix)
    pub meta_artist: String,
    /// Track title extracted from metadata (title or icy-title suffix)
    pub meta_track: String,
    pub volume: f64,
    pub paused: bool,
    pub muted: bool,
    /// Audio bitrate in kbps (0 = unknown)
    pub bitrate_kbps: u32,
}

pub type SharedRadioState = Arc<Mutex<RadioState>>;

// ─── IPC Handle ──────────────────────────────────────────────────────────────

/// Returned by spawn_ipc_task — used to send commands to mpv.
pub struct IpcHandle {
    tx: tokio::sync::mpsc::Sender<String>,
}

impl IpcHandle {
    fn send_cmd(&self, cmd: String) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(cmd).await;
        });
    }

    pub fn set_volume(&self, vol: f64) {
        let v = vol.clamp(0.0, 100.0);
        self.send_cmd(format!(r#"{{"command":["set_property","volume",{}]}}"#, v));
    }

    pub fn toggle_pause(&self) {
        self.send_cmd(r#"{"command":["cycle","pause"]}"#.into());
    }

    pub fn set_mute(&self, muted: bool) {
        let val = if muted { "yes" } else { "no" };
        self.send_cmd(format!(
            r#"{{"command":["set_property","mute","{}"],"request_id":1}}"#,
            val
        ));
    }

    pub fn quit(&self) {
        self.send_cmd(r#"{"command":["quit"]}"#.into());
    }
}

// ─── Background Task ─────────────────────────────────────────────────────────

/// Spawns a tokio task that:
///   1. Waits for the socket to appear (up to 2s)
///   2. Reads property-change events and updates shared state
///   3. Forwards outgoing commands from the channel to mpv
pub fn spawn_ipc_task(socket_path: String, state: SharedRadioState) -> IpcHandle {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
    let state_clone = state.clone();

    tokio::spawn(async move {
        // Wait for socket to appear
        let deadline = Instant::now() + Duration::from_secs(2);
        let stream = loop {
            match UnixStream::connect(&socket_path).await {
                Ok(s) => break Some(s),
                Err(_) => {
                    if Instant::now() >= deadline {
                        break None;
                    }
                    sleep(Duration::from_millis(100)).await;
                }
            }
        };

        let stream = match stream {
            Some(s) => s,
            None => return, // socket never appeared — mpv probably failed
        };

        let (reader_half, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader_half).lines();

        // Subscribe to properties we need
        let subs = [
            r#"{"command":["observe_property",1,"volume"]}"#,
            r#"{"command":["observe_property",2,"pause"]}"#,
            r#"{"command":["observe_property",3,"mute"]}"#,
            r#"{"command":["observe_property",4,"media-title"]}"#,
            r#"{"command":["observe_property",5,"metadata"]}"#,
            r#"{"command":["observe_property",6,"audio-bitrate"]}"#,
        ];
        for sub in &subs {
            let msg = format!("{}\n", sub);
            if writer.write_all(msg.as_bytes()).await.is_err() {
                return;
            }
        }

        loop {
            tokio::select! {
                // Incoming event from mpv
                line = timeout(Duration::from_millis(500), lines.next_line()) => {
                    match line {
                        Ok(Ok(Some(text))) => handle_event(&text, &state_clone),
                        Ok(Ok(None)) => break, // socket closed
                        Ok(Err(_)) | Err(_) => {} // timeout or IO error — keep trying
                    }
                }
                // Outgoing command from TUI
                cmd = rx.recv() => {
                    if let Some(cmd) = cmd {
                        let msg = format!("{}\n", cmd);
                        if writer.write_all(msg.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    IpcHandle { tx }
}

// ─── Event Parser ─────────────────────────────────────────────────────────────

fn handle_event(text: &str, state: &SharedRadioState) {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    if v.get("event").and_then(|e| e.as_str()) != Some("property-change") {
        return;
    }

    let name = match v.get("name").and_then(|n| n.as_str()) {
        Some(n) => n,
        None => return,
    };
    let data = &v["data"];

    let mut st = state.lock().unwrap();
    match name {
        "volume" => {
            if let Some(vol) = data.as_f64() {
                st.volume = vol;
            }
        }
        "pause" => {
            if let Some(p) = data.as_bool() {
                st.paused = p;
            }
        }
        "mute" => {
            // mpv sends mute as string "yes"/"no" or bool depending on version
            if let Some(b) = data.as_bool() {
                st.muted = b;
            } else if let Some(s) = data.as_str() {
                st.muted = s == "yes";
            }
        }
        "media-title" => {
            if let Some(t) = data.as_str() {
                st.media_title = t.to_string();
            }
        }
        "metadata" => {
            // Station name
            if let Some(name) = data.get("icy-name").and_then(|v| v.as_str()) {
                st.icy_name = name.to_string();
            }

            // Raw icy-title (usually "Artist - Track")
            let raw_icy = data
                .get("icy-title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            st.icy_title = raw_icy.clone();

            // Prefer explicit artist/title tags if present
            let explicit_artist = data.get("artist").and_then(|v| v.as_str()).unwrap_or("");
            let explicit_title = data
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !explicit_artist.is_empty() || !explicit_title.is_empty() {
                st.meta_artist = explicit_artist.to_string();
                st.meta_track = explicit_title.to_string();
            } else if !raw_icy.is_empty() {
                // Parse "Artist - Track" from icy-title
                if let Some((artist, track)) = raw_icy.split_once(" - ") {
                    st.meta_artist = artist.trim().to_string();
                    st.meta_track = track.trim().to_string();
                } else {
                    // Whole icy-title is the track, no artist known
                    st.meta_artist.clear();
                    st.meta_track = raw_icy;
                }
            }
        }
        "audio-bitrate" => {
            // mpv reports bitrate in bits/s as float
            if let Some(bps) = data.as_f64() {
                st.bitrate_kbps = (bps / 1000.0).round() as u32;
            }
        }
        _ => {}
    }
}

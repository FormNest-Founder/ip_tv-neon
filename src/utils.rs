use chrono::{DateTime, NaiveDateTime, Utc};
use std::fs::OpenOptions;
use std::io::Write;

pub fn main_log(msg: &str) {
    let log_path = crate::consts::get_cache_dir().join("neon_iptv.log");
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| writeln!(f, "[{}] {}", Utc::now().format("%H:%M:%S"), msg));
}

pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Strip terminal control characters from untrusted text before it reaches any
/// TTY/ratatui sink (CG8). Replacing every C0/C1 control char — including ESC
/// (0x1B) and BEL (0x07) — with a space neutralizes ANSI CSI / OSC sequences:
/// once the introducer ESC is gone the trailing bytes are inert printable text.
pub fn sanitize_terminal(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

pub fn parse_xml_time(s: &str) -> i64 {
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y%m%d%H%M%S %z") {
        return dt.timestamp();
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M%S") {
        return dt.and_utc().timestamp();
    }
    0
}

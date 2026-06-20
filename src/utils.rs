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

pub fn parse_xml_time(s: &str) -> i64 {
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y%m%d%H%M%S %z") {
        return dt.timestamp();
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M%S") {
        return dt.and_utc().timestamp();
    }
    0
}

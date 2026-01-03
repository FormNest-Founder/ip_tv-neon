use chrono::{DateTime, Utc};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub fn get_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("neon-iptv")
}

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";
pub const RADIO_API: &str = "https://www.radiorecord.ru/api/stations";
pub const RADIO_NOW_API: &str = "https://www.radiorecord.ru/api/stations/now";
pub const RECOMMENDED_EPG: &str = "http://epg.one/epg.xml.gz";

pub fn main_log(msg: &str) {
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/neon_iptv.log")
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
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y%m%d%H%M%S") {
        return dt.timestamp();
    }
    0
}

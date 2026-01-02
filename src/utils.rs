use chrono::{DateTime, Utc};
use std::fs::OpenOptions;
use std::io::Write;

pub const CACHE_DIR: &str = "neon_cache";
pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";
pub const RADIO_API: &str = "https://www.radiorecord.ru/api/stations";
pub const RADIO_NOW_API: &str = "https://www.radiorecord.ru/api/stations/now";
pub const RECOMMENDED_EPG: &str = "http://epg.one/edem_epg_ico2.m3u8";

pub fn main_log(msg: &str) {
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/neon_iptv.log")
        .and_then(|mut f| writeln!(f, "[{}] {}", Utc::now().format("%H:%M:%S"), msg));
}

pub fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

pub fn parse_xml_time(s: &str) -> i64 {
    // Try to parse with timezone first: "20231027120000 +0300"
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y%m%d%H%M%S %z") {
        return dt.timestamp();
    }
    // Fallback without timezone (assume UTC)
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y%m%d%H%M%S") {
        return dt.timestamp();
    }
    0
}
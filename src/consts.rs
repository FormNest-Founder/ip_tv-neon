use std::path::PathBuf;

pub const APP_VERSION: u32 = 911;
pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";
pub const RADIO_API: &str = "https://www.radiorecord.ru/api/stations";
pub const RADIO_NOW_API: &str = "https://www.radiorecord.ru/api/stations/now";
pub const RECOMMENDED_EPG: &str = "https://epg.one/epg.xml.gz";

pub fn get_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("neon-iptv")
}

pub fn get_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("neon-iptv")
}

pub fn get_data_bin_path() -> PathBuf {
    get_cache_dir().join("data.bin")
}

pub fn get_config_json_path() -> PathBuf {
    get_config_dir().join("config.json")
}

use std::path::PathBuf;

#[allow(dead_code)] // keep for diagnostics: app version displayed in --help / future use
pub const APP_VERSION: u32 = 911;
/// Cache schema version — bump when bincode struct layout changes.
/// Policy: increment any time CacheContainer or AppData fields change.
/// Independent of APP_VERSION so cache invalidations don't require app version bump.
pub const CACHE_SCHEMA_VERSION: u32 = 2;
pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";
pub const RADIO_API: &str = "https://www.radiorecord.ru/api/stations/";
pub const RADIO_NOW_API: &str = "https://www.radiorecord.ru/api/stations/now/";
pub const RECOMMENDED_EPG: &str = "https://epg.one/epg.xml.gz";

// ─── Ingest Hard Caps (CG2 — decompression-bomb / OOM defence) ───────────────
// The EPG/playlist URLs are user-editable and point at third-party servers, so
// every byte read from them is bounded before allocation.

/// Max compressed/raw bytes accepted from an EPG download.
pub const MAX_EPG_DOWNLOAD_BYTES: usize = 128 * 1024 * 1024;
/// Max bytes fed to the XML parser after gzip decompression. Sized with headroom
/// over the real default source (epg.one ≈ 417 MB decompressed, 2026-06) so
/// legitimate data is never truncated; a true bomb is still bounded here.
pub const MAX_EPG_DECOMPRESSED_BYTES: u64 = 768 * 1024 * 1024;
/// Max `<programme>` entries kept from one EPG payload.
pub const MAX_EPG_PROGRAMMES: usize = 4_000_000;
/// Max distinct channel display-names kept from one EPG payload.
pub const MAX_EPG_CHANNELS: usize = 200_000;
/// Max bytes accepted from a remote M3U playlist download.
pub const MAX_PLAYLIST_BYTES: usize = 64 * 1024 * 1024;
/// Max channels parsed from one playlist.
pub const MAX_CHANNELS: usize = 500_000;

// ─── AGY CLI Backend ─────────────────────────────────────────────────────────

/// Wall-clock kill timeout for the agy subprocess (CG3).
pub const AGY_TIMEOUT_SECS: u64 = 90;
/// Preferred agy binary location; PATH is used as a fallback.
pub const AGY_PREFERRED_PATH: &str = "/home/admin/.local/bin/agy";

pub fn get_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| {
            // Per-UID fallback so other local users cannot access our data.
            let uid = unsafe { libc::getuid() };
            let fallback = PathBuf::from(format!("/tmp/neon-iptv-{uid}"));
            eprintln!(
                "[neon-iptv] XDG config dir unavailable, using {}",
                fallback.display()
            );
            fallback
        })
        .join("neon-iptv")
}

pub fn get_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| {
            let uid = unsafe { libc::getuid() };
            let fallback = PathBuf::from(format!("/tmp/neon-iptv-{uid}"));
            eprintln!(
                "[neon-iptv] XDG cache dir unavailable, using {}",
                fallback.display()
            );
            fallback
        })
        .join("neon-iptv")
}

pub fn get_data_bin_path() -> PathBuf {
    get_cache_dir().join("data.bin")
}

pub fn get_config_json_path() -> PathBuf {
    get_config_dir().join("config.json")
}

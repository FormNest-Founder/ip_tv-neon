use std::collections::{HashMap, HashSet};

use ip_tv_neon::models::Config;

#[test]
fn config_save_load_roundtrip() {
    let original = Config {
        playlist_url: "http://example.com/playlist.m3u".into(),
        epg_url: "http://example.com/epg.xml".into(),
        theme_color: (100, 200, 50),
        favorites: {
            let mut s = HashSet::new();
            s.insert("http://fav1.com".into());
            s.insert("http://fav2.com".into());
            s
        },
        history: vec!["http://hist1.com".into(), "http://hist2.com".into()],
        channel_names: {
            let mut m = HashMap::new();
            m.insert("http://ch1.com".into(), "Channel One".into());
            m
        },
        video_fullscreen: true,
        video_geometry: "1920x1080".into(),
        local_dir: String::new(),
        llm_provider: "deepseek".into(),
    };

    let json = serde_json::to_string_pretty(&original).expect("serialize");
    let restored: Config = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(original.playlist_url, restored.playlist_url);
    assert_eq!(original.epg_url, restored.epg_url);
    assert_eq!(original.theme_color, restored.theme_color);
    assert_eq!(original.favorites, restored.favorites);
    assert_eq!(original.history, restored.history);
    assert_eq!(original.channel_names, restored.channel_names);
    assert_eq!(original.video_fullscreen, restored.video_fullscreen);
    assert_eq!(original.video_geometry, restored.video_geometry);
    assert_eq!(original.llm_provider, restored.llm_provider);
}

#[test]
fn config_save_uses_atomic_rename() {
    let source = include_str!("../src/models.rs");
    assert!(
        source.contains(r#"with_extension("#),
        "Config::save must use temp file"
    );
    assert!(
        source.contains("fs::rename"),
        "Config::save must use atomic rename"
    );
    assert!(
        source.contains("sync_all"),
        "Config::save must fsync before rename"
    );
}

#[test]
fn corrupt_config_is_backed_up_before_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    std::fs::write(&path, b"{ this is not valid json ]").expect("write corrupt");

    let cfg = Config::load_from(&path);

    // Falls back to defaults instead of propagating the parse error.
    assert_eq!(cfg.playlist_url, Config::default().playlist_url);
    // The corrupt file is preserved as a .bak, NOT left in place to be
    // overwritten by the next save().
    let bak = path.with_extension("json.bak");
    assert!(bak.exists(), "corrupt config must be backed up to .bak");
    assert!(!path.exists(), "corrupt config must be moved out of the way");
    assert_eq!(
        std::fs::read(&bak).unwrap(),
        b"{ this is not valid json ]"
    );
}

#[test]
fn valid_config_loads_from_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    let cfg = Config {
        playlist_url: "http://example.com/list.m3u".into(),
        ..Config::default()
    };
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    std::fs::write(&path, json).expect("write valid");

    let loaded = Config::load_from(&path);
    assert_eq!(loaded.playlist_url, "http://example.com/list.m3u");
    // A valid file is left untouched (no spurious .bak).
    assert!(!path.with_extension("json.bak").exists());
}

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
        source.contains(r#"with_extension("tmp")"#),
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

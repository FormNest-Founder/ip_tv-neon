use std::collections::HashMap;

use ip_tv_neon::epg::find_epg_id;
use ip_tv_neon::models::{AppData, Channel, EpgProgram};

fn make_channel(tvg_id: Option<&str>, norm_name: &str) -> Channel {
    Channel {
        name: String::new(),
        group: String::new(),
        url: String::new(),
        tvg_id: tvg_id.map(String::from),
        norm_name: norm_name.into(),
        catchup_days: 0,
        name_lower: String::new(),
    }
}

fn make_data(epg_keys: &[&str], name_to_id: &[(&str, &str)]) -> AppData {
    let epg: HashMap<String, Vec<EpgProgram>> = epg_keys
        .iter()
        .map(|k| {
            (
                k.to_string(),
                vec![EpgProgram {
                    start: 0,
                    stop: 100,
                    title: "Test".into(),
                    desc: String::new(),
                }],
            )
        })
        .collect();
    let name_to_id: HashMap<String, String> = name_to_id
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    AppData {
        channels: Vec::new(),
        radio: Vec::new(),
        radio_groups: Vec::new(),
        groups: Vec::new(),
        epg,
        name_to_id,
        group_counts: HashMap::new(),
        ..Default::default()
    }
}

#[test]
fn find_epg_id_tvg_id_match() {
    let ch = make_channel(Some("bbc.one"), "BBC One");
    let data = make_data(&["bbc.one", "itv.one"], &[]);
    let id = find_epg_id(&ch, &data);
    assert_eq!(id, Some("bbc.one".into()));
}

#[test]
fn find_epg_id_norm_fallback() {
    let ch = make_channel(None, "bbc one");
    let data = make_data(&[], &[("bbc one", "bbc.one")]);
    let id = find_epg_id(&ch, &data);
    assert_eq!(id, Some("bbc.one".into()));
}

#[test]
fn find_epg_id_no_match() {
    let ch = make_channel(None, "unknown");
    let data = make_data(&[], &[]);
    let id = find_epg_id(&ch, &data);
    assert_eq!(id, None);
}

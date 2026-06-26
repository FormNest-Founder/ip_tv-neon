use ip_tv_neon::ai::{
    extract_keywords, next_choice_id, resolve_agy_binary, resolve_choice, search_epg, Backend,
    MODEL_CATALOG,
};
use ip_tv_neon::models::{AppData, Channel, EpgProgram};

#[test]
fn extract_keywords_strips_prefix() {
    let input = "Here is the answer.\nKEYWORDS: боевик, комедия, x\nMore text.";
    let (clean, kw) = extract_keywords(input);
    assert!(!clean.contains("KEYWORDS:"));
    assert!(clean.contains("Here is the answer"));
    let kw_list = kw.expect("should have keywords");
    assert!(kw_list.contains(&"боевик".to_string()));
    assert!(kw_list.contains(&"комедия".to_string()));
    assert!(!kw_list.contains(&"x".to_string())); // single char filtered
}

#[test]
fn extract_keywords_case_insensitive() {
    let input = "Some text.\nkeywords: action, drama\nMore.";
    let (clean, kw) = extract_keywords(input);
    assert!(!clean.contains("keywords:"));
    let kw_list = kw.expect("should have keywords");
    assert!(kw_list.contains(&"action".to_string()));
    assert!(kw_list.contains(&"drama".to_string()));
}

#[test]
fn extract_keywords_no_keywords() {
    let input = "Just regular text without keywords.";
    let (clean, kw) = extract_keywords(input);
    assert_eq!(clean, "Just regular text without keywords.");
    assert!(kw.is_none());
}

// ─── Model Catalog ────────────────────────────────────────────────────────────

#[test]
fn catalog_ids_unique() {
    let mut ids: Vec<&str> = MODEL_CATALOG.iter().map(|c| c.id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "MODEL_CATALOG ids must be unique");
}

#[test]
fn next_choice_id_wraps() {
    let first = MODEL_CATALOG[0].id;
    let last = MODEL_CATALOG[MODEL_CATALOG.len() - 1].id;
    assert_eq!(next_choice_id(last), first, "last wraps back to first");

    // Cycling through every row returns to the starting id.
    let mut id = first;
    for _ in 0..MODEL_CATALOG.len() {
        id = next_choice_id(id);
    }
    assert_eq!(id, first, "full cycle returns to first");
}

#[test]
fn resolve_legacy_gemini() {
    let c = resolve_choice("gemini");
    assert_eq!(c.id, "gemini");
    assert_eq!(c.backend, Backend::Gemini);
}

#[test]
fn resolve_unknown_defaults_to_first() {
    assert_eq!(resolve_choice("nonsense-xyz").id, MODEL_CATALOG[0].id);
    assert_eq!(resolve_choice("").id, MODEL_CATALOG[0].id);
    assert_eq!(resolve_choice("deepseek").id, MODEL_CATALOG[0].id);
    assert_eq!(MODEL_CATALOG[0].backend, Backend::DeepSeek);
}

#[test]
fn agy_binary_unknown_is_none() {
    // Nonexistent preferred path + empty/absent PATH → no binary found.
    assert!(resolve_agy_binary("/nonexistent/path/agy-xyz", Some("")).is_none());
    assert!(resolve_agy_binary("/nonexistent/path/agy-xyz", None).is_none());
}

// ─── EPG Search Relevance ──────────────────────────────────────────────────────

fn chan(name: &str, tvg: &str) -> Channel {
    Channel {
        name: name.into(),
        group: "Movies".into(),
        url: format!("http://example/{tvg}"),
        tvg_id: Some(tvg.into()),
        norm_name: name.to_lowercase(),
        catchup_days: 0,
        name_lower: name.to_lowercase(),
    }
}

fn prog(title: &str, desc: &str, now: i64) -> EpgProgram {
    EpgProgram {
        start: now,
        stop: now + 3600,
        title: title.into(),
        desc: desc.into(),
    }
}

/// Reproduces the relevance bug: a substring keyword ("оно") used to match inside
/// "регИОНОв" (cooking show) and a lone description keyword used to pull in an
/// unrelated action film. The fix (whole-word matching + title-or-≥2-desc gate)
/// must exclude both while keeping a real horror whose title carries the keyword.
#[test]
fn search_epg_excludes_substring_and_lone_desc_false_positives() {
    let now = 1_700_000_000;
    let channels = vec![
        chan("Пятница! +2", "fri"),
        chan("Боевик HD", "act"),
        chan("Ужасы HD", "hor"),
    ];
    let mut epg = std::collections::HashMap::new();
    // Cooking show: "регионов" contains the substring "оно" — must NOT match.
    epg.insert(
        "fri".to_string(),
        vec![prog(
            "Секреты на кухне. Кухня разных регионов",
            "Кулинарное шоу о кухнях регионов России.",
            now,
        )],
    );
    // Action film: a single horror keyword in the description only — must NOT match.
    epg.insert(
        "act".to_string(),
        vec![prog(
            "Джон Уик 3",
            "Легендарный убийца снова берётся за дело.",
            now,
        )],
    );
    // Real horror: keyword appears as a whole word in the title — MUST match.
    epg.insert(
        "hor".to_string(),
        vec![prog("Заклятие 2", "Семья сталкивается с проклятием.", now)],
    );

    let data = AppData {
        channels,
        epg,
        ..Default::default()
    };
    let keywords: Vec<String> = ["оно", "убийца", "заклятие"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let titles: Vec<String> = search_epg(&data, &keywords, now)
        .into_iter()
        .map(|r| r.program.title)
        .collect();

    assert!(
        titles.iter().any(|t| t == "Заклятие 2"),
        "real horror must be retained, got {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t.contains("Секреты на кухне")),
        "cooking show matched via substring 'оно' in 'регионов' must be excluded, got {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t == "Джон Уик 3"),
        "action film with a lone desc keyword must be excluded, got {titles:?}"
    );
}

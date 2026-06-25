use ip_tv_neon::ai::{
    extract_keywords, next_choice_id, resolve_agy_binary, resolve_choice, Backend, MODEL_CATALOG,
};

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

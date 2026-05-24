use ip_tv_neon::ai::extract_keywords;

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

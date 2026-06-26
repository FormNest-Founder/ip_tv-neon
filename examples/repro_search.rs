// TEMPORARY multi-genre evidence harness. Not shipped. Runs the real agy prompt
// for a query (argv), then classifies the OLD (buggy substring) vs NEW (fixed)
// result sets against the real cache, flagging cross-genre junk and channel-name
// collisions. Usage: repro_search <query words...>

use ip_tv_neon::ai::{extract_keywords, load_system_prompt, search_epg};
use ip_tv_neon::epg::{find_epg_id, load_data};
use ip_tv_neon::models::AppData;

// Re-expose the boundary check used by the fix so classification matches it.
fn word_contains(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    for (idx, _) in haystack.match_indices(needle) {
        let before_ok = haystack[..idx]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after = idx + needle.len();
        let after_ok = haystack[after..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Replicates the ORIGINAL buggy search (raw `contains`, score>=1, raw channel
/// name-fallback). Returns (channel_name, title, desc) per result.
fn old_search(data: &AppData, kw: &[String], now: i64) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for ch in &data.channels {
        let mut hit = false;
        if let Some(id) = find_epg_id(ch, data) {
            if let Some(progs) = data.epg.get(&id) {
                for p in progs {
                    if p.stop < now - 86400 * 7 {
                        continue;
                    }
                    let t = p.title.to_lowercase();
                    let d = p.desc.to_lowercase();
                    let th = kw.iter().filter(|k| t.contains(k.as_str())).count();
                    let dh = kw
                        .iter()
                        .filter(|k| !t.contains(k.as_str()) && d.contains(k.as_str()))
                        .count();
                    if th * 2 + dh >= 1 {
                        hit = true;
                        out.push((ch.name.clone(), p.title.clone(), p.desc.clone()));
                    }
                }
            }
        }
        if !hit {
            let ns = kw.iter().filter(|k| ch.name_lower.contains(k.as_str())).count();
            if ns > 0 {
                out.push((ch.name.clone(), format!("[Channel] {}", ch.name), String::new()));
            }
        }
    }
    out.truncate(100);
    out
}

/// Classify one result by the fixed semantics: title hit / >=2 desc / channel
/// fallback / weak (the junk class the fix targets).
fn classify(title: &str, desc: &str, kw: &[String]) -> &'static str {
    if title.starts_with("[Channel] ") {
        return "channel";
    }
    let t = title.to_lowercase();
    let d = desc.to_lowercase();
    let th = kw.iter().filter(|k| word_contains(&t, k)).count();
    let dh = kw
        .iter()
        .filter(|k| !word_contains(&t, k) && word_contains(&d, k))
        .count();
    if th >= 1 {
        "title"
    } else if dh >= 2 {
        "desc2"
    } else {
        "weak"
    }
}

#[tokio::main]
async fn main() {
    let query: String = {
        let a: Vec<String> = std::env::args().skip(1).collect();
        if a.is_empty() { "подборка ужастиков".into() } else { a.join(" ") }
    };

    // Replay mode: "--kw a,b,c" skips agy and uses the given keywords verbatim.
    let keywords: Vec<String> = if let Some(rest) = query.strip_prefix("--kw ") {
        rest.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()
    } else {
        let system = load_system_prompt().await;
        let prompt = format!("{system}\n\nUSER: {query}\nASSISTANT:");
        let out = std::process::Command::new("/home/admin/.local/bin/agy")
            .args(["-p", &prompt, "--model", "gemini-3.5-flash", "--print-timeout", "90s"])
            .output()
            .expect("agy failed");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let (_c, kw) = extract_keywords(&stdout);
        kw.unwrap_or_default()
    };

    let data = load_data();
    let now = chrono::Utc::now().timestamp();

    println!("════════════════════════════════════════════════════════");
    println!("QUERY: {query}");
    println!("KEYWORDS ({}): {:?}", keywords.len(), keywords);

    // OLD
    let old = old_search(&data, &keywords, now);
    let mut o = std::collections::BTreeMap::new();
    for (_, t, d) in &old {
        *o.entry(classify(t, d, &keywords)).or_insert(0) += 1;
    }
    println!("OLD: total={} classes={:?}", old.len(), o);

    // NEW
    let new = search_epg(&data, &keywords, now);
    let mut n = std::collections::BTreeMap::new();
    for r in &new {
        *n.entry(classify(&r.program.title, &r.program.desc, &keywords)).or_insert(0) += 1;
    }
    println!("NEW: total={} classes={:?}", new.len(), n);

    // Show NEW non-title results (the collision-risk ones) + a sample of titles.
    println!("-- NEW desc2/channel results (collision-risk) --");
    for r in &new {
        let c = classify(&r.program.title, &r.program.desc, &keywords);
        if c == "channel" || c == "desc2" || c == "weak" {
            println!("   [{c}] [{}] {}", r.channel_name, r.program.title);
        }
    }
    println!("-- NEW sample (first 18 titles) --");
    for r in new.iter().take(18) {
        println!("   [{}] {}", r.channel_name, r.program.title);
    }
}

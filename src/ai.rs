use crate::epg::{find_epg_id, get_current_epg};
use crate::models::{AppData, Channel, EpgProgram};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Result of AI-powered EPG search
#[derive(Clone, Debug)]
pub struct AiSearchResult {
    pub channel_idx: usize,
    pub program: EpgProgram,
    pub channel_name: String,
    pub score: u32,
    pub is_live: bool,
    pub has_archive: bool,
}

/// Chat message for display
#[derive(Clone, Debug)]
pub struct ChatMsg {
    pub is_user: bool,
    pub text: String,
}

/// Response from AI chat: text + optional keywords for EPG search
pub struct AiChatResponse {
    pub text: String,
    pub keywords: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    messages: Vec<ApiMessage>,
    temperature: f32,
}

#[derive(Deserialize)]
struct ApiChoice {
    message: ApiMsgResp,
}

#[derive(Deserialize)]
struct ApiMsgResp {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct ApiResponse {
    choices: Vec<ApiChoice>,
}

const SYSTEM_PROMPT: &str = "\
Ты — NEON AI, персональный ТВ-ассистент и эксперт по кино/сериалам.

ТВОИ ВОЗМОЖНОСТИ:
- Рекомендовать фильмы и сериалы из ТЕКУЩЕГО ЭФИРА (секция NOW_PLAYING)
- Подбирать контент по жанру, настроению, актёрам, режиссёру, рейтингу
- Учитывать историю просмотра пользователя для персональных рекомендаций
- Описывать сюжет, давать оценку, сравнивать фильмы

ПРАВИЛА:
1. Отвечай ТОЛЬКО на русском, кратко (2-5 предложений).
2. При рекомендациях СНАЧАЛА ищи подходящее в NOW_PLAYING (текущий эфир).
3. Используй свои знания о кино (рейтинги, жанры, актёры) для подбора.
4. Для ПОИСКА добавь последней строкой: KEYWORDS: название1, название2, название3
   KEYWORDS должны содержать КОНКРЕТНЫЕ названия фильмов/сериалов (русские или оригинальные).
   Каждое keyword — отдельное название или ключевая часть названия.
   НЕ используй общие слова (фильм, сериал, кино, лучший, рейтинг, топ).
5. Если пользователь просто общается — НЕ добавляй KEYWORDS.
6. Если в NOW_PLAYING нет подходящего — скажи об этом, предложи альтернативу из эфира.
7. Учитывай HISTORY — не рекомендуй то, что пользователь уже смотрит постоянно (если он не просит).";

/// Build context string with current EPG + viewing history
pub fn build_context(data: &AppData, config_history: &[String], channels: &[Channel]) -> String {
    let now = chrono::Utc::now().timestamp();
    let mut ctx = String::with_capacity(8000);

    // Viewing history — last 15 unique channel names
    if !config_history.is_empty() {
        ctx.push_str("=== HISTORY (recently watched) ===\n");
        let mut seen = std::collections::HashSet::new();
        let mut count = 0;
        for url in config_history.iter().rev() {
            if let Some(ch) = channels.iter().find(|c| c.url == *url) {
                if seen.insert(&ch.name) {
                    ctx.push_str(&ch.name);
                    ctx.push('\n');
                    count += 1;
                    if count >= 15 { break; }
                }
            }
        }
        ctx.push('\n');
    }

    // Current EPG — what's playing NOW across all channels
    ctx.push_str("=== NOW_PLAYING (current broadcast) ===\n");
    let mut epg_count = 0;
    for ch in channels {
        if let Some(prog) = get_current_epg(ch, data, now) {
            let archive = if ch.catchup_days > 0 { " [archive]" } else { "" };
            ctx.push_str(&format!("{}: {}{}\n", ch.name, prog.title, archive));
            epg_count += 1;
        }
        if epg_count >= 200 { break; }
    }

    ctx
}

/// Full chat with DeepSeek — returns response text + optional search keywords
pub async fn ai_chat(
    client: &Client,
    history: &[ChatMsg],
    user_msg: &str,
    context: &str,
) -> AiChatResponse {
    let api_key = match std::env::var("DEEPSEEK_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => return AiChatResponse {
            text: "Error: DEEPSEEK_API_KEY not set in /etc/environment".into(),
            keywords: None,
        },
    };

    // Build system message with context
    let system_content = if context.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{}\n\n{}", SYSTEM_PROMPT, context)
    };

    let mut messages = vec![ApiMessage {
        role: "system".into(),
        content: system_content,
    }];

    // Add conversation history (last 10 messages to save tokens)
    let skip = if history.len() > 10 { history.len() - 10 } else { 0 };
    for msg in &history[skip..] {
        messages.push(ApiMessage {
            role: if msg.is_user { "user" } else { "assistant" }.into(),
            content: msg.text.clone(),
        });
    }

    messages.push(ApiMessage {
        role: "user".into(),
        content: user_msg.into(),
    });

    let body = ApiRequest {
        model: "deepseek-chat",
        messages,
        temperature: 0.7,
    };

    let resp = match client
        .post("https://api.deepseek.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(30))
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return AiChatResponse {
            text: format!("Network error: {}", e),
            keywords: None,
        },
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return AiChatResponse {
            text: format!("API error {}: {}", status, &body[..body.len().min(200)]),
            keywords: None,
        };
    }

    let parsed: ApiResponse = match resp.json().await {
        Ok(p) => p,
        Err(e) => return AiChatResponse {
            text: format!("Parse error: {}", e),
            keywords: None,
        },
    };

    let full_text = parsed
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .cloned()
        .unwrap_or_else(|| "No response".into());

    // Extract KEYWORDS line if present
    let (display_text, keywords) = extract_keywords(&full_text);

    AiChatResponse {
        text: display_text,
        keywords,
    }
}

/// Extract "KEYWORDS: ..." line from response, return (clean_text, Option<keywords>)
fn extract_keywords(text: &str) -> (String, Option<Vec<String>>) {
    let mut lines: Vec<&str> = Vec::new();
    let mut keywords: Option<Vec<String>> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(kw_str) = trimmed.strip_prefix("KEYWORDS:").or_else(|| trimmed.strip_prefix("keywords:")) {
            let kw: Vec<String> = kw_str
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| s.len() >= 2)
                .filter(|s| !STOP_WORDS.contains(&s.as_str()))
                .collect();
            if !kw.is_empty() {
                keywords = Some(kw);
            }
        } else {
            lines.push(line);
        }
    }

    // Trim trailing empty lines
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }

    (lines.join("\n"), keywords)
}

const STOP_WORDS: &[&str] = &[
    "фильм", "фильмы", "сериал", "сериалы", "кино", "передача", "канал",
    "список", "рейтинг", "лучший", "лучшие", "новый", "новые", "главное",
    "выпуск", "программа", "эфир", "серия", "сезон", "смотреть", "онлайн",
    "movie", "film", "show", "series", "best", "new", "episode", "channel",
    "watch", "online", "top", "rating", "про", "the", "and",
];

/// Search EPG across all channels using keywords
pub fn search_epg(data: &AppData, keywords: &[String], now: i64) -> Vec<AiSearchResult> {
    let kw_lower: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).collect();
    let mut results: Vec<AiSearchResult> = Vec::new();

    for (ch_idx, ch) in data.channels.iter().enumerate() {
        let epg_id = find_epg_id(ch, data);
        let mut has_epg_match = false;

        if let Some(ref id) = epg_id {
            if let Some(programs) = data.epg.get(id) {
                for prog in programs {
                    if prog.stop < now - 86400 * 7 {
                        continue;
                    }

                    let title_lower = prog.title.to_lowercase();
                    let desc_lower = prog.desc.to_lowercase();

                    let title_hits: u32 = kw_lower
                        .iter()
                        .filter(|kw| title_lower.contains(kw.as_str()))
                        .count() as u32;
                    let desc_hits: u32 = kw_lower
                        .iter()
                        .filter(|kw| !title_lower.contains(kw.as_str()) && desc_lower.contains(kw.as_str()))
                        .count() as u32;
                    let score = title_hits * 2 + desc_hits;

                    if score >= 1 {
                        has_epg_match = true;
                        results.push(AiSearchResult {
                            channel_idx: ch_idx,
                            program: prog.clone(),
                            channel_name: ch.name.clone(),
                            score,
                            is_live: now >= prog.start && now < prog.stop,
                            has_archive: ch.catchup_days > 0 && prog.stop <= now,
                        });
                    }
                }
            }
        }

        if !has_epg_match {
            let name_score: u32 = kw_lower
                .iter()
                .filter(|kw| ch.name_lower.contains(kw.as_str()))
                .count() as u32;

            if name_score > 0 {
                results.push(AiSearchResult {
                    channel_idx: ch_idx,
                    program: EpgProgram {
                        start: 0,
                        stop: 0,
                        title: format!("[Channel] {}", ch.name),
                        desc: String::new(),
                    },
                    channel_name: ch.name.clone(),
                    score: name_score,
                    is_live: false,
                    has_archive: false,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        b.is_live
            .cmp(&a.is_live)
            .then(b.score.cmp(&a.score))
            .then(a.program.start.cmp(&b.program.start))
    });

    results.dedup_by(|a, b| {
        a.channel_idx == b.channel_idx
            && a.program.title == b.program.title
            && a.program.start == b.program.start
    });

    results.truncate(100);
    results
}

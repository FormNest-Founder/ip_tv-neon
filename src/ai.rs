// ─── Imports ─────────────────────────────────────────────────────────────────

use crate::epg::{find_epg_id, get_current_epg};
use crate::models::{AppData, Channel, EpgProgram};
use reqwest::Client;
use serde::{Deserialize, Serialize};

// ─── Data Types ──────────────────────────────────────────────────────────────

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

// ─── LLM Provider ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum LlmProvider {
    DeepSeek,
    Gemini,
}

impl LlmProvider {
    pub fn name(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::Gemini => "Gemini",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::DeepSeek => Self::Gemini,
            Self::Gemini => Self::DeepSeek,
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "gemini" => Self::Gemini,
            _ => Self::DeepSeek,
        }
    }
}

// ─── API Types (OpenAI-compatible: DeepSeek) ────────────────────────────────

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

// ─── API Types (Gemini) ─────────────────────────────────────────────────────

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction")]
    system_instruction: GeminiContent,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenConfig,
}

#[derive(Serialize)]
struct GeminiGenConfig {
    temperature: f32,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiRespContent>,
}

#[derive(Deserialize)]
struct GeminiRespContent {
    parts: Option<Vec<GeminiRespPart>>,
}

#[derive(Deserialize)]
struct GeminiRespPart {
    text: Option<String>,
}

// ─── System Prompt ───────────────────────────────────────────────────────────

const DEFAULT_PROMPT: &str = "\
Ты — NEON AI, персональный ТВ-ассистент и эксперт по кино/сериалам.

КАК РАБОТАЕТ ПОИСК:
Когда ты добавляешь строку KEYWORDS — система АВТОМАТИЧЕСКИ ищет по ВСЕЙ базе EPG \
(сотни каналов, тысячи программ на неделю вперёд, включая архив). \
Тебе НЕ нужно вручную искать в NOW_PLAYING — просто дай правильные ключевые слова, \
и поисковый движок найдёт ВСЁ подходящее.

ПРАВИЛА:
1. Отвечай ТОЛЬКО на русском, кратко (2-4 предложения).
2. ВСЕГДА добавляй KEYWORDS когда пользователь ищет, просит подборку или рекомендацию.
3. Формат последней строки: KEYWORDS: слово1, слово2, слово3, слово4, слово5
   - Для жанровых запросов: ключевые слова жанра + 5-10 известных названий фильмов этого жанра.
   - Для конкретных запросов: названия фильмов (русские И оригинальные).
   - Чем БОЛЬШЕ keywords — тем лучше поиск. Давай 5-15 штук.
4. НЕ используй слишком общие слова (фильм, сериал, кино, лучший, канал, рейтинг).
5. Если пользователь просто общается (не ищет контент) — НЕ добавляй KEYWORDS.
6. Учитывай HISTORY для персональных рекомендаций.
7. NOW_PLAYING — справка о текущем эфире. Используй для ответов 'что сейчас идёт'.
8. Ты ЭКСПЕРТ по кино и сериалам. Используй свои знания о рейтингах, актёрах, режиссёрах, жанрах, наградах.";

/// Load system prompt from ~/.config/neon-iptv/ai_prompt.md (fallback to built-in default)
pub async fn load_system_prompt() -> String {
    let path = crate::consts::get_config_dir().join("ai_prompt.md");
    tokio::fs::read_to_string(&path)
        .await
        .unwrap_or_else(|_| DEFAULT_PROMPT.to_string())
}

// ─── Context Builder ─────────────────────────────────────────────────────────

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
                    if count >= 15 {
                        break;
                    }
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
            let archive = if ch.catchup_days > 0 {
                " [archive]"
            } else {
                ""
            };
            ctx.push_str(&format!("{}: {}{}\n", ch.name, prog.title, archive));
            epg_count += 1;
        }
        if epg_count >= 200 {
            break;
        }
    }

    ctx
}

// ─── AI Chat ─────────────────────────────────────────────────────────────────

/// Chat with LLM (DeepSeek or Gemini) — returns response text + optional search keywords
pub async fn ai_chat(
    client: &Client,
    history: &[ChatMsg],
    user_msg: &str,
    context: &str,
    provider: LlmProvider,
) -> AiChatResponse {
    let prompt = load_system_prompt().await;
    let system_content = if context.is_empty() {
        prompt
    } else {
        format!("{}\n\n{}", prompt, context)
    };

    let full_text = match provider {
        LlmProvider::DeepSeek => chat_deepseek(client, history, user_msg, &system_content).await,
        LlmProvider::Gemini => chat_gemini(client, history, user_msg, &system_content).await,
    };

    let full_text = match full_text {
        Ok(t) => t,
        Err(e) => {
            return AiChatResponse {
                text: e,
                keywords: None,
            }
        }
    };

    let (display_text, keywords) = extract_keywords(&full_text);
    AiChatResponse {
        text: display_text,
        keywords,
    }
}

async fn chat_deepseek(
    client: &Client,
    history: &[ChatMsg],
    user_msg: &str,
    system: &str,
) -> Result<String, String> {
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or("DEEPSEEK_API_KEY not set in /etc/environment")?;

    let mut messages = vec![ApiMessage {
        role: "system".into(),
        content: system.into(),
    }];
    let skip = history.len().saturating_sub(10);
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
    let resp = client
        .post("https://api.deepseek.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(30))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "API error {}: {}",
            status,
            &body[..body.len().min(200)]
        ));
    }

    let parsed: ApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;
    Ok(parsed
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .cloned()
        .unwrap_or_else(|| "No response".into()))
}

async fn chat_gemini(
    client: &Client,
    history: &[ChatMsg],
    user_msg: &str,
    system: &str,
) -> Result<String, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or("GEMINI_API_KEY not set in /etc/environment")?;

    let mut contents = Vec::new();
    let skip = history.len().saturating_sub(10);
    for msg in &history[skip..] {
        contents.push(GeminiContent {
            role: if msg.is_user { "user" } else { "model" }.into(),
            parts: vec![GeminiPart {
                text: msg.text.clone(),
            }],
        });
    }
    contents.push(GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart {
            text: user_msg.into(),
        }],
    });

    let body = GeminiRequest {
        contents,
        system_instruction: GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart {
                text: system.into(),
            }],
        },
        generation_config: GeminiGenConfig {
            temperature: 0.7,
            max_output_tokens: 2048,
        },
    };

    let resp = client
        .post("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent")
        .header("X-Goog-Api-Key", &api_key)
        .timeout(std::time::Duration::from_secs(30))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Gemini API error {}: {}",
            status,
            &body[..body.len().min(200)]
        ));
    }

    let parsed: GeminiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;
    Ok(parsed
        .candidates
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.content.as_ref())
        .and_then(|c| c.parts.as_ref())
        .and_then(|p| p.first())
        .and_then(|p| p.text.as_ref())
        .cloned()
        .unwrap_or_else(|| "No response".into()))
}

// ─── Keywords Extraction ─────────────────────────────────────────────────────

/// Extract "KEYWORDS: ..." line from response, return (clean_text, Option<keywords>)
pub fn extract_keywords(text: &str) -> (String, Option<Vec<String>>) {
    let mut lines: Vec<&str> = Vec::new();
    let mut keywords: Option<Vec<String>> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(kw_str) = trimmed
            .strip_prefix("KEYWORDS:")
            .or_else(|| trimmed.strip_prefix("keywords:"))
        {
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
    "фильм",
    "фильмы",
    "сериал",
    "сериалы",
    "кино",
    "передача",
    "канал",
    "список",
    "рейтинг",
    "лучший",
    "лучшие",
    "новый",
    "новые",
    "главное",
    "выпуск",
    "программа",
    "эфир",
    "серия",
    "сезон",
    "смотреть",
    "онлайн",
    "movie",
    "film",
    "show",
    "series",
    "best",
    "new",
    "episode",
    "channel",
    "watch",
    "online",
    "top",
    "rating",
    "про",
    "the",
    "and",
];

// ─── EPG Search ──────────────────────────────────────────────────────────────

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
                        .filter(|kw| {
                            !title_lower.contains(kw.as_str()) && desc_lower.contains(kw.as_str())
                        })
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

    // Dedup first: sort by dedup key, remove duplicates, then sort by display order
    results.sort_by(|a, b| {
        a.channel_idx
            .cmp(&b.channel_idx)
            .then(a.program.title.cmp(&b.program.title))
            .then(a.program.start.cmp(&b.program.start))
    });
    results.dedup_by(|a, b| {
        a.channel_idx == b.channel_idx
            && a.program.title == b.program.title
            && a.program.start == b.program.start
    });
    results.sort_by(|a, b| {
        b.is_live
            .cmp(&a.is_live)
            .then(b.score.cmp(&a.score))
            .then(a.program.start.cmp(&b.program.start))
    });

    results.truncate(100);
    results
}

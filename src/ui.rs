// ─── Imports ─────────────────────────────────────────────────────────────────

use crate::app::App;
use crate::player::VU_BARS;
use crate::epg::get_current_epg;
use crate::models::{Channel, Screen, SETTINGS_COUNT, SETTINGS_LABELS};
use crate::utils::sanitize_terminal as sanitize;
use chrono::Utc;
use ratatui::{prelude::*, widgets::*};

// ─── Neon Palette ─────────────────────────────────────────────────────────────

const NEON_CYAN: Color = Color::Rgb(0, 255, 229);
const NEON_MAGENTA: Color = Color::Rgb(255, 0, 200);
const NEON_YELLOW: Color = Color::Rgb(255, 220, 0);
#[allow(dead_code)]
const NEON_DIM: Color = Color::Rgb(40, 0, 60);

// ─── Radio player height (compact, AIMP-style) ───────────────────────────────

/// Height of the compact neon radio widget in lines (including its border).
/// VU rows = RADIO_PANEL_H - 7 (border×2 + station + track + vu + controls + hint)
const RADIO_PANEL_H: u16 = 13;

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn get_name_by_url<'a>(
    url: &'a str,
    channels: &'a [Channel],
    config: &'a crate::models::Config,
) -> &'a str {
    if let Some(ch) = channels.iter().find(|ch| ch.url == url) {
        return ch.name.as_str();
    }
    config.channel_name(url)
}

/// Return a `width`-char window of `text` starting at `offset`, with looping via
/// a 2-space separator. Safe for any terminal width.
fn marquee_slice(text: &str, offset: usize, width: usize) -> String {
    if width == 0 || text.is_empty() {
        return " ".repeat(width);
    }
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if len <= width {
        let mut s: String = chars.iter().collect();
        while s.chars().count() < width {
            s.push(' ');
        }
        return s;
    }
    // Loop: text + "  " separator
    let total = len + 2;
    let start = offset % total;
    let mut out = String::with_capacity(width);
    for i in 0..width {
        let idx = (start + i) % total;
        out.push(if idx < len { chars[idx] } else { ' ' });
    }
    out
}

static CATEGORY_ICONS: &[(&[&str], &str)] = &[
    (&["usa", "сша", "america"], "🇺🇸"),
    (&["belarus", "белар"], "🇧🇾"),
    (&["russia", "росси", "рф"], "🇷🇺"),
    (&["ukrain", "украин"], "🇺🇦"),
    (&["kazakh", "казах"], "🇰🇿"),
    (&["uk ", "^uk", "british", "англ"], "🇬🇧"),
    (&["german", "немец", "deutsch"], "🇩🇪"),
    (&["france", "франц", "french"], "🇫🇷"),
    (&["italy", "итал", "italian"], "🇮🇹"),
    (&["spain", "испан", "spanish"], "🇪🇸"),
    (&["turkey", "турц", "türk"], "🇹🇷"),
    (&["india", "инди", "hindi"], "🇮🇳"),
    (&["china", "кита", "chinese"], "🇨🇳"),
    (&["japan", "япон"], "🇯🇵"),
    (&["korea", "коре"], "🇰🇷"),
    (&["arab", "араб"], "🇸🇦"),
    (&["israel", "израил"], "🇮🇱"),
    (&["poland", "поль", "polsk"], "🇵🇱"),
    (&["czech", "чеш"], "🇨🇿"),
    (&["canada", "канад"], "🇨🇦"),
    (&["brazil", "бразил"], "🇧🇷"),
    (&["georgia", "грузи"], "🇬🇪"),
    (&["armenia", "армен"], "🇦🇲"),
    (&["azerbai", "азерб"], "🇦🇿"),
    (&["uzbek", "узбек"], "🇺🇿"),
    (&["moldov", "молдов"], "🇲🇩"),
    (&["latin", "латин"], "🌎"),
    (&["europe", "европ"], "🌍"),
    (&["asia", "азия"], "🌏"),
    (&["internat", "междунар", "world", "мир"], "🌐"),
    (&["кино", "фильм", "movie", "cinema"], "🎬"),
    (&["сериал", "series"], "🎭"),
    (&["мульт", "cartoon", "kids", "детск", "child"], "🧸"),
    (&["спорт", "sport", "football", "футбол"], "⚽"),
    (&["новост", "news"], "📰"),
    (&["музык", "music"], "🎵"),
    (&["наук", "science", "discovery", "nat geo"], "🔬"),
    (&["document", "докум"], "📚"),
    (&["образов", "educat"], "🎓"),
    (&["религ", "relig", "духов"], "🕊 "),
    (&["эротик", "adult", "xxx", "18+"], "🔞"),
    (&["travel", "путеш"], "✈ "),
    (&["кулинар", "cook", "food", "еда"], "🍳"),
    (&["fashion", "мода", "style"], "👗"),
    (&["humor", "юмор", "comedy", "комед"], "😂"),
    (&["horror", "ужас"], "👻"),
    (&["познав", "develop"], "💡"),
    (&["shop", "магаз", "телемаг"], "🛒"),
    (&["radost", "радост"], "🌈"),
    (&["retro", "ретро", "classic", "классик", "совет"], "📽 "),
    (&["hd", "uhd", "4k"], "📺"),
];

fn category_icon(name: &str) -> &'static str {
    let n = name.to_lowercase();
    for &(keywords, icon) in CATEGORY_ICONS {
        if keywords.iter().any(|&k| {
            if let Some(prefix) = k.strip_prefix('^') {
                n.starts_with(prefix)
            } else {
                n.contains(k)
            }
        }) {
            return icon;
        }
    }
    "📂"
}

static RADIO_GENRE_ICONS: &[(&[&str], &str)] = &[
    (&["bass", "dubstep", "drum", "dnb"], "🔊"),
    (&["rock", "metal", "punk", "grunge"], "🎸"),
    (&["pop", "dance", "disco"], "🎤"),
    (&["jazz", "soul", "blues", "funk"], "🎷"),
    (&["classic", "класси", "orchestra"], "🎻"),
    (&["electro", "techno", "trance", "house", "edm"], "🎧"),
    (&["hip", "rap", "trap", "phonk"], "🎤"),
    (&["chill", "lounge", "ambient", "relax"], "🌊"),
    (&["reggae", "ska", "dub"], "🌴"),
    (&["country", "folk"], "🤠"),
    (&["latin", "salsa", "reggaeton"], "💃"),
    (&["russian", "русск", "рус"], "🇷🇺"),
    (&["hit", "top", "best", "gold"], "🏆"),
    (&["remix", "mashup", "mix"], "🔀"),
    (&["retro", "80", "90", "70", "old"], "📼"),
    (&["new", "fresh", "нов"], "✨"),
    (&["deep"], "🌀"),
    (&["pirate"], "🏴‍☠️"),
    (&["summer"], "☀ "),
    (&["party", "club", "superdiskoteka", "дискотека"], "🪩"),
    (&["vip", "premium"], "💎"),
    (&["record"], "⏺ "),
    (&["chanson", "шансон"], "🎶"),
    (&["naft", "нафт"], "💧"),
    (&["big"], "🔥"),
    (&["melodi", "мелоди"], "🎵"),
    (&["humor", "юмор", "сказк", "comedy"], "😂"),
];

fn radio_genre_icon(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n == "all" {
        return "📻";
    }
    for &(keywords, icon) in RADIO_GENRE_ICONS {
        if keywords.iter().any(|&k| n.contains(k)) {
            return icon;
        }
    }
    "🎵"
}

// ─── Main Render Dispatch ────────────────────────────────────────────────────

pub fn ui(f: &mut Frame, app: &mut App) {
    let size = f.area();
    let (r, g, b) = app.config.theme_color;
    let theme = Color::Rgb(r, g, b);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" NIGHT CITY HUB ")
        .border_style(Style::default().fg(theme));
    f.render_widget(block.clone(), size);
    let full_area = block.inner(size);

    // For TV — full-screen "now playing" overlay (mpv has a window, TUI is blocked)
    if app.player.mpv_handle.is_some() && app.player.radio_ipc.is_none() {
        let text = "\n\n\n  ▶  NOW PLAYING\n\n  Press ESC to stop";
        f.render_widget(
            Paragraph::new(text)
                .alignment(Alignment::Center)
                .fg(theme)
                .bold(),
            full_area,
        );
        return;
    }

    // When radio is playing: split screen — list on top, compact neon player below.
    // The list area adapts: only RadioList and RadioCatList actually make sense above
    // the player, but other screens remain unaffected.
    let (content_area, radio_panel_area) =
        if app.player.radio_ipc.is_some() && full_area.height > RADIO_PANEL_H + 3 {
            let chunks = Layout::default()
                .constraints([Constraint::Min(3), Constraint::Length(RADIO_PANEL_H)])
                .split(full_area);
            (chunks[0], Some(chunks[1]))
        } else {
            (full_area, None)
        };

    // ── Screen Dispatch ───────────────────────────────────────────────────
    match &app.screen {
        Screen::Updating => {
            let text = "\n\n  UPDATING DATA...\n  PLEASE WAIT...";
            f.render_widget(
                Paragraph::new(text)
                    .alignment(Alignment::Center)
                    .fg(Color::Yellow)
                    .bold(),
                content_area,
            );
        }

        Screen::MainMenu => {
            let has_status = app.status_msg.is_some();
            let constraints = if has_status {
                vec![
                    Constraint::Length(10),
                    Constraint::Min(0),
                    Constraint::Length(3),
                ]
            } else {
                vec![Constraint::Length(10), Constraint::Min(0)]
            };
            let chunks = Layout::default()
                .constraints(constraints)
                .split(content_area);
            let version = env!("CARGO_PKG_VERSION");
            let status = format!(
                "   NEON HUB v{}\n   Channels: {}  Radio: {}",
                version,
                app.data.channels.len(),
                app.data.radio.len()
            );
            f.render_widget(
                Paragraph::new(status)
                    .alignment(Alignment::Center)
                    .fg(theme),
                chunks[0],
            );
            let items = [
                "  📺  IPTV",
                "  📻  RADIO",
                "  📁  LOCAL",
                "  🤖  AI CHAT",
                "  ⭐  FAVORITES",
                "  🕐  HISTORY",
                "  ⏹   STOP ALL",
                "  🔄  UPDATE",
                "  ⚙   SETTINGS",
                "  🚪  EXIT",
            ];
            let list = List::new(items.map(|s| ListItem::new(s).style(Style::default().fg(theme))))
                .highlight_style(Style::default().bg(theme).fg(Color::Black).bold());
            f.render_stateful_widget(list, chunks[1], &mut app.nav.m_state);
            if let Some(msg) = &app.status_msg {
                let color = if msg.starts_with("Update failed") {
                    Color::Red
                } else {
                    Color::Green
                };
                f.render_widget(
                    Paragraph::new(format!(" {}", msg)).fg(color).block(
                        Block::default()
                            .borders(Borders::TOP)
                            .border_style(Style::default().fg(Color::DarkGray)),
                    ),
                    chunks[2],
                );
            }
        }

        Screen::CatList => {
            let items: Vec<ListItem> = app
                .data
                .groups
                .iter()
                .map(|g| {
                    let cnt = app.data.group_counts.get(g).copied().unwrap_or(0);
                    ListItem::new(format!("  {}  {} ({})", category_icon(g), g, cnt))
                        .style(Style::default().fg(theme))
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" 📺 Categories ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme)),
                )
                .highlight_style(Style::default().bg(theme).fg(Color::Black).bold());
            f.render_stateful_widget(list, content_area, &mut app.nav.cat_state);
        }

        Screen::ChanList => {
            let chunks = Layout::default()
                .constraints([Constraint::Min(0), Constraint::Length(3)])
                .split(content_area);
            let now = Utc::now().timestamp();
            let items: Vec<ListItem> = app
                .nav
                .filtered
                .iter()
                .map(|&idx| {
                    let ch = &app.data.channels[idx];
                    let is_fav = app.config.favorites.contains(&ch.url);
                    let has_archive = ch.catchup_days > 0;
                    let mut spans: Vec<Span> = Vec::new();
                    if is_fav {
                        spans.push(Span::styled("★ ", Style::default().fg(Color::Yellow)));
                    }
                    if has_archive {
                        spans.push(Span::styled(
                            "⏪",
                            Style::default().fg(Color::Rgb(100, 140, 255)),
                        ));
                    }
                    if !is_fav && !has_archive {
                        spans.push(Span::raw("  "));
                    }
                    spans.push(Span::styled(&ch.name, Style::default().fg(Color::White)));
                    if let Some(p) = get_current_epg(ch, &app.data, now) {
                        let pct = if p.stop > p.start {
                            ((now - p.start) as f64 / (p.stop - p.start) as f64).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let filled = (pct * 12.0) as usize;
                        let bar: String = (0..12)
                            .map(|i| if i < filled { '▰' } else { '▱' })
                            .collect();
                        let bar_color = if pct < 0.3 {
                            Color::Rgb(0, 200, 120)
                        } else if pct < 0.7 {
                            Color::Rgb(200, 200, 0)
                        } else {
                            Color::Rgb(255, 100, 60)
                        };
                        spans.push(Span::styled(
                            format!("  {}", bar),
                            Style::default().fg(bar_color),
                        ));
                        spans.push(Span::styled(
                            format!(" {}", p.title),
                            Style::default().fg(Color::Rgb(180, 140, 255)),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let title = format!(" {} ({}) ", app.nav.selected_group, app.nav.filtered.len());
            let list = List::new(items)
                .block(Block::default().title(title).borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .bg(Color::Rgb(0, 40, 40))
                        .fg(Color::Cyan)
                        .bold(),
                );
            f.render_stateful_widget(list, chunks[0], &mut app.nav.ch_state);
            f.render_widget(
                Paragraph::new(format!(" SEARCH: {}", app.nav.search)).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
                chunks[1],
            );
        }

        Screen::RadioCatList => {
            let items: Vec<ListItem> = app
                .data
                .radio_groups
                .iter()
                .map(|g| {
                    let cnt = if g == "All" {
                        app.data.radio.len()
                    } else {
                        app.data
                            .radio
                            .iter()
                            .filter(|r| r.genres.contains(g))
                            .count()
                    };
                    ListItem::new(format!("  {}  {} ({})", radio_genre_icon(g), g, cnt))
                        .style(Style::default().fg(theme))
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" 📻 Radio Genres ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme)),
                )
                .highlight_style(Style::default().bg(theme).fg(Color::Black).bold());
            f.render_stateful_widget(list, content_area, &mut app.nav.r_cat_state);
        }

        Screen::RadioList => {
            let items: Vec<ListItem> = app
                .nav
                .filtered_radio
                .iter()
                .map(|&idx| {
                    let st = &app.data.radio[idx];
                    let track = st.track.as_deref().unwrap_or("");
                    let mut spans =
                        vec![Span::styled(&st.title, Style::default().fg(Color::White))];
                    if !track.is_empty() {
                        spans.push(Span::styled(
                            format!("  {}", track),
                            Style::default().fg(Color::Green),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let title = format!(
                " Radio: {} ({}) ",
                app.nav.selected_radio_genre,
                app.nav.filtered_radio.len()
            );
            let list = List::new(items)
                .block(Block::default().title(title).borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .bg(Color::Rgb(0, 30, 0))
                        .fg(Color::Green)
                        .bold(),
                );
            f.render_stateful_widget(list, content_area, &mut app.nav.r_state);
        }

        Screen::Favorites => {
            let favs = app.sorted_favorites();
            let items: Vec<ListItem> = favs
                .iter()
                .map(|url| {
                    let name = get_name_by_url(url, &app.data.channels, &app.config);
                    ListItem::new(format!("  ⭐  {}", name)).style(Style::default().fg(theme))
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" ⭐ Favorites ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme)),
                )
                .highlight_style(Style::default().bg(theme).fg(Color::Black).bold());
            f.render_stateful_widget(list, content_area, &mut app.nav.fav_state);
        }

        Screen::History => {
            let items: Vec<ListItem> = app
                .config
                .history
                .iter()
                .rev()
                .map(|url| {
                    let name = get_name_by_url(url, &app.data.channels, &app.config);
                    ListItem::new(format!("  🕐  {}", name)).style(Style::default().fg(theme))
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" 🕐 History ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme)),
                )
                .highlight_style(Style::default().bg(theme).fg(Color::Black).bold());
            f.render_stateful_widget(list, content_area, &mut app.nav.hist_state);
        }

        Screen::Settings => {
            render_settings(f, app, content_area, None);
        }

        Screen::SettingsEdit(field) => {
            render_settings(f, app, content_area, Some(*field));
        }

        Screen::LocalList => {
            let items: Vec<ListItem> = app
                .local_files
                .iter()
                .map(|p| {
                    ListItem::new(format!("  📄  {}", p.to_string_lossy()))
                        .style(Style::default().fg(theme))
                })
                .collect();
            let dir_label = if app.config.local_dir.is_empty() {
                "~/".to_string()
            } else {
                app.config.local_dir.clone()
            };
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(format!(" 📁 Local Playlists — {} ", dir_label))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme)),
                )
                .highlight_style(Style::default().bg(theme).fg(Color::Black).bold());
            f.render_stateful_widget(list, content_area, &mut app.nav.d_state);
        }

        Screen::Detail => {
            render_detail(f, app, content_area);
        }

        Screen::AiChat => {
            render_ai_chat(f, app, content_area, theme);
        }

        Screen::LinkInput => {
            f.render_widget(
                Paragraph::new("\n\n  Not yet implemented.\n  Press ESC to return.")
                    .alignment(Alignment::Center),
                content_area,
            );
        }
    }

    // ── Compact neon radio panel (bottom) ─────────────────────────────────
    if let Some(panel_area) = radio_panel_area {
        render_radio_panel(f, app, panel_area);
    }
}

// ─── Compact NEON RADIO Panel ────────────────────────────────────────────────

fn render_radio_panel(f: &mut Frame, app: &App, area: Rect) {
    // Snapshot IPC state (lock once, release before rendering)
    let (paused, muted, volume, media_title, icy_name, meta_artist, meta_track, bitrate_kbps, connected) = {
        let st = app.player.radio_state.lock().unwrap_or_else(|e| e.into_inner());
        (
            st.paused,
            st.muted,
            st.volume,
            st.media_title.clone(),
            st.icy_name.clone(),
            st.meta_artist.clone(),
            st.meta_track.clone(),
            st.bitrate_kbps,
            st.connected,
        )
    };

    let status_icon = if muted {
        "🔇"
    } else if paused {
        "⏸"
    } else {
        "▶"
    };
    let play_icon = if paused { "▶ " } else { "⏸" };
    let mute_icon = if muted { "🔇" } else { "🔊" };
    let status_color = if paused || muted {
        Color::Rgb(100, 100, 120)
    } else {
        Color::Rgb(0, 255, 130)
    };

    let bitrate_str = if bitrate_kbps > 0 {
        format!(" ♪ {}kbps", bitrate_kbps)
    } else {
        " ♪ ---".to_string()
    };

    // Outer neon border with title
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(NEON_CYAN))
        .title(Line::from(vec![
            Span::styled(
                " ░▒▓█ NEON RADIO █▓▒░",
                Style::default().fg(NEON_CYAN).bold(),
            ),
            Span::styled(bitrate_str, Style::default().fg(NEON_MAGENTA)),
            Span::raw(" "),
        ]));
    f.render_widget(outer.clone(), area);
    let inner = outer.inner(area);

    // Guard: need at least 5 rows
    if inner.height < 5 {
        return;
    }

    // Layout: station(1) + marquee(1) + vu(flex) + controls(1) + hint(1)
    let vu_h = inner.height.saturating_sub(4).max(1);
    let chunks = Layout::default()
        .constraints([
            Constraint::Length(1),    // station name
            Constraint::Length(1),    // track marquee
            Constraint::Length(vu_h), // VU meters
            Constraint::Length(1),    // controls + volume slider
            Constraint::Length(1),    // hint (separate line — no wrapping)
        ])
        .split(inner);

    // ── Station line ──────────────────────────────────────────────────────
    // Prefer icy-name from stream metadata, fall back to playlist title
    let station_display = if !connected {
        "IPC not connected…".to_string()
    } else if !icy_name.is_empty() {
        sanitize(&icy_name)
    } else {
        sanitize(&app.player.radio_station_title)
    };
    let inner_w = inner.width.saturating_sub(4) as usize;
    let station_w = inner_w.saturating_sub(4); // leave room for " ▶  " prefix

    let station_line = Line::from(vec![
        Span::styled(
            format!(" {} ", status_icon),
            Style::default().fg(status_color).bold(),
        ),
        Span::styled(
            marquee_slice(&station_display, app.player.visuals.marquee_offset / 3, station_w),
            Style::default()
                .fg(Color::Rgb(0, 240, 255))
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(station_line), chunks[0]);

    // ── Track marquee: "Station │ Artist │ Track" ─────────────────────────
    // Build the composite marquee string from available metadata pieces.
    let safe_artist = sanitize(&meta_artist);
    let safe_track = sanitize(&meta_track);
    let marquee_text = build_marquee_text(
        &app.player.radio_station_title,
        &safe_artist,
        &safe_track,
        &media_title,
    );
    let track_line = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            marquee_slice(
                &marquee_text,
                app.player.visuals.marquee_offset,
                inner_w.saturating_sub(2),
            ),
            Style::default().fg(Color::Rgb(200, 220, 255)),
        ),
    ]);
    f.render_widget(Paragraph::new(track_line), chunks[1]);

    // ── VU Meters ─────────────────────────────────────────────────────────
    render_vu_meters(f, app, chunks[2]);

    // ── Controls + Volume slider ───────────────────────────────────────────
    // Reserve chars: " ◀◀ ▶  ■ 🔊  VOL ●" = ~18, " XXX%  " = 6 → vol_w = rest
    let vol_w = (inner.width as usize).saturating_sub(26).max(4);
    let pct = (volume / 100.0).clamp(0.0, 1.0);
    let filled = (pct * vol_w as f64) as usize;
    let bar_color = lerp_color((0, 255, 229), (255, 0, 200), pct as f32);
    let filled_str = "━".repeat(filled);
    let empty_str = "─".repeat(vol_w.saturating_sub(filled));

    let ctrl_line = Line::from(vec![
        Span::styled(
            format!(" ◀◀ {} ■ {}  ", play_icon, mute_icon),
            Style::default().fg(NEON_CYAN).bold(),
        ),
        Span::styled("VOL ", Style::default().fg(Color::Rgb(140, 160, 200))),
        Span::styled("●", Style::default().fg(NEON_CYAN)),
        Span::styled(filled_str, Style::default().fg(bar_color)),
        Span::styled(empty_str, Style::default().fg(Color::Rgb(35, 35, 55))),
        Span::styled(
            format!(" {:.0}%", volume),
            Style::default()
                .fg(Color::Rgb(220, 240, 255))
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(ctrl_line), chunks[3]);

    // ── Adaptive hint (own line — no truncation possible via overflow) ────
    let hint = adaptive_hint(inner.width);
    f.render_widget(
        Paragraph::new(hint).fg(Color::Rgb(120, 140, 180)),
        chunks[4],
    );
}

/// Build the composite marquee text: "Station │ Artist │ Track"
/// Skips empty segments and joins remaining ones with " │ ".
/// `artist`/`track` arrive already sanitized; `station` and `media_title` are
/// sanitized here so untrusted stream/playlist metadata can't reach the TTY (CG8).
fn build_marquee_text(station: &str, artist: &str, track: &str, media_title: &str) -> String {
    let station = sanitize(station);
    // If we have structured metadata — build rich format
    if !track.is_empty() {
        let mut parts: Vec<&str> = vec![station.as_str()];
        if !artist.is_empty() {
            parts.push(artist);
        }
        parts.push(track);
        return parts.join(" │ ");
    }
    // Fall back to media_title (mpv's best guess)
    if !media_title.is_empty() {
        return format!("{} │ {}", station, sanitize(media_title));
    }
    // Nothing — just station name (marquee will scroll if long)
    station
}

/// Return a hint string that fits into `width` terminal columns.
fn adaptive_hint(width: u16) -> &'static str {
    if width >= 72 {
        "  Space:⏸/▶   + / -:Vol   M:🔇   ↑↓:Station   Enter:Play   Esc:■ Stop"
    } else if width >= 52 {
        "  Sp:⏸  ±:Vol  M:🔇  ↑↓:Sta  Esc:■"
    } else {
        "  ⏸ ± 🔇 ↑↓ ■"
    }
}

// ─── VU Meters ───────────────────────────────────────────────────────────────

fn render_vu_meters(f: &mut Frame, app: &App, area: Rect) {
    let usable_w = area.width.saturating_sub(2) as usize;
    let n_bars = (usable_w / 2).clamp(1, VU_BARS);
    let bar_h = area.height as usize;
    if bar_h == 0 || n_bars == 0 {
        return;
    }

    let block_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut lines: Vec<Line> = Vec::with_capacity(bar_h);

    for row in 0..bar_h {
        // row 0 = top, row bar_h-1 = bottom
        let row_threshold = 1.0 - (row as f32 / bar_h as f32);
        let row_lower = 1.0 - ((row + 1) as f32 / bar_h as f32);

        let mut spans = vec![Span::raw(" ")];
        for i in 0..n_bars {
            let h = app.player.visuals.vu_bars[i];
            let peak = app.player.visuals.vu_peaks[i];

            // Peak indicator: show ▔ in the row where peak sits
            let peak_row_f = (1.0 - peak) * bar_h as f32;
            let peak_row = peak_row_f as usize;
            let is_peak_row = peak_row == row && peak > 0.01;

            let ch = if is_peak_row {
                // Peak dot overrides bar content
                '▔'
            } else if h >= row_threshold {
                '█'
            } else {
                let frac = if row_threshold > row_lower {
                    (h - row_lower) / (row_threshold - row_lower)
                } else {
                    0.0
                };
                if frac > 0.01 {
                    let idx = ((frac * 8.0) as usize).min(7);
                    block_chars[idx]
                } else {
                    ' '
                }
            };

            let color = if is_peak_row {
                NEON_YELLOW
            } else {
                vu_color(row, bar_h)
            };
            spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
            spans.push(Span::raw(" "));
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// VU color: top row = yellow/orange, mid = magenta, bottom = cyan
fn vu_color(row: usize, total_rows: usize) -> Color {
    if total_rows == 0 {
        return NEON_CYAN;
    }
    let t = row as f32 / total_rows as f32; // 0=top, 1=bottom
    if t < 0.25 {
        let blend = t / 0.25;
        lerp_color((255, 220, 0), (255, 0, 200), blend)
    } else if t < 0.6 {
        NEON_MAGENTA
    } else {
        let blend = (t - 0.6) / 0.4;
        lerp_color((255, 0, 200), (0, 255, 229), blend)
    }
}

fn lerp_color(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::Rgb(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}

// ─── Detail Screen ───────────────────────────────────────────────────────────

fn render_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let ch_idx = match app.detail.channel {
        Some(i) => i,
        None => return,
    };
    let ch = match app.data.channels.get(ch_idx) {
        Some(c) => c,
        None => return, // channel list changed under us — nothing to render
    };
    let now = Utc::now().timestamp();
    let (r, g, b) = app.config.theme_color;
    let theme = Color::Rgb(r, g, b);
    let is_fav = app.config.favorites.contains(&ch.url);

    let chunks = Layout::default()
        .constraints([
            Constraint::Length(4),
            Constraint::Percentage(45),
            Constraint::Percentage(45),
            Constraint::Length(1),
        ])
        .split(area);

    let fav_marker = if is_fav { " ★" } else { "" };
    let archive_info = if ch.catchup_days > 0 {
        format!("  Archive: {} days", ch.catchup_days)
    } else {
        String::new()
    };
    let header = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            format!(" {}{}", &ch.name, fav_marker),
            Style::default().fg(theme).bold(),
        )]),
        Line::from(vec![Span::styled(
            format!(" {}{}", ch.group, archive_info),
            Style::default().fg(Color::DarkGray),
        )]),
    ])
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(header, chunks[0]);

    if app.detail.programs.is_empty() {
        f.render_widget(
            Paragraph::new("  No EPG data available")
                .fg(Color::DarkGray)
                .block(Block::default().title(" Programs ").borders(Borders::ALL)),
            chunks[1],
        );
    } else {
        let items: Vec<ListItem> = app
            .detail
            .programs
            .iter()
            .map(|p| {
                let is_current = now >= p.start && now < p.stop;
                let is_past = p.stop <= now;
                let time_str = format_time(p.start);
                let end_str = format_time(p.stop);
                let marker = if is_current {
                    "▶ "
                } else if is_past && ch.catchup_days > 0 {
                    "⏪"
                } else {
                    "  "
                };
                let title_style = if is_current {
                    Style::default().fg(Color::Green).bold()
                } else if is_past {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                };
                let time_style = if is_current {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Cyan)
                };
                let line = Line::from(vec![
                    Span::styled(
                        format!(" {}", marker),
                        Style::default().fg(if is_current {
                            Color::Green
                        } else if is_past && ch.catchup_days > 0 {
                            Color::Yellow
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled(format!("{}-{} ", time_str, end_str), time_style),
                    Span::styled(&p.title, title_style),
                ]);
                ListItem::new(line)
            })
            .collect();

        let title = if ch.catchup_days > 0 {
            " Programs (⏪ = archive) ".to_string()
        } else {
            " Programs ".to_string()
        };
        let list = List::new(items)
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(0, 40, 40))
                    .fg(Color::Cyan)
                    .bold(),
            );
        f.render_stateful_widget(list, chunks[1], &mut app.nav.epg_state);
    }

    let desc_text = if let Some(idx) = app.nav.epg_state.selected() {
        if idx < app.detail.programs.len() {
            let p = &app.detail.programs[idx];
            if p.desc.is_empty() {
                p.title.clone()
            } else {
                format!("{}\n{}", p.title, p.desc)
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    f.render_widget(
        Paragraph::new(format!(" {}", desc_text))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(" Description ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        chunks[2],
    );
    f.render_widget(
        Paragraph::new(" Enter: play | L: live | F: fav | ESC: back ").fg(Color::DarkGray),
        chunks[3],
    );
}

// ─── Time Formatting ─────────────────────────────────────────────────────────

fn format_time(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_default()
}

// ─── Settings Screen ─────────────────────────────────────────────────────────

fn render_settings(f: &mut Frame, app: &mut App, area: Rect, editing: Option<usize>) {
    let (r, g, b) = app.config.theme_color;
    let theme = Color::Rgb(r, g, b);
    let chunks = Layout::default()
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let items: Vec<ListItem> = (0..SETTINGS_COUNT)
        .map(|i| {
            let label = SETTINGS_LABELS[i];
            let val = app.settings_value(i);
            let is_editing = editing == Some(i);
            let display_val = if is_editing { &app.nav.edit_buf } else { &val };
            let style = if is_editing {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            let marker = match i {
                2 | 4 | 5 | 7 | 8 => " [Enter: toggle]",
                _ => " [Enter: edit]",
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {}: ", label), Style::default().fg(theme).bold()),
                Span::styled(display_val.to_string(), style),
                Span::styled(marker, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" ⚙  Settings ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme)),
        )
        .highlight_style(Style::default().bg(theme).fg(Color::Black).bold());
    f.render_stateful_widget(list, chunks[0], &mut app.nav.set_state);

    let hint = if editing.is_some() {
        " Type to edit | Enter: save | ESC: cancel "
    } else if let Some(msg) = &app.status_msg {
        msg.as_str()
    } else {
        " Up/Down: navigate | Enter: edit/toggle | ESC: back "
    };
    f.render_widget(
        Paragraph::new(hint).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        chunks[1],
    );
}

// ─── AI Chat Screen ──────────────────────────────────────────────────────────

fn render_ai_chat(f: &mut Frame, app: &mut App, area: Rect, theme: Color) {
    let focus_border = |focused: bool| -> Style {
        if focused {
            Style::default().fg(theme)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };

    let main_chunks = Layout::default()
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    if app.ai.results.is_empty() {
        let hint = if app.ai.chat_history.is_empty() {
            "  Ask something — results will appear here"
        } else {
            "  No results for this query"
        };
        f.render_widget(
            Paragraph::new(hint).fg(Color::DarkGray).block(
                Block::default()
                    .title(" Results ")
                    .borders(Borders::ALL)
                    .border_style(focus_border(false)),
            ),
            main_chunks[0],
        );
    } else {
        let now = Utc::now().timestamp();
        let items: Vec<ListItem> = app
            .ai
            .results
            .iter()
            .map(|r| {
                let mut spans: Vec<Span> = Vec::new();
                if r.is_live {
                    spans.push(Span::styled(
                        " LIVE ",
                        Style::default().fg(Color::Black).bg(Color::Green).bold(),
                    ));
                    spans.push(Span::raw(" "));
                } else if r.has_archive {
                    spans.push(Span::styled(
                        " REC ",
                        Style::default().fg(Color::Black).bg(Color::Yellow).bold(),
                    ));
                    spans.push(Span::raw(" "));
                } else if r.program.start > now {
                    spans.push(Span::styled("  ▷  ", Style::default().fg(Color::Cyan)));
                } else {
                    spans.push(Span::raw("     "));
                }
                spans.push(Span::styled(
                    &r.channel_name,
                    Style::default().fg(Color::Cyan).bold(),
                ));
                spans.push(Span::raw("  "));
                if r.program.start > 0 {
                    spans.push(Span::styled(
                        format!(
                            "{}-{}",
                            format_time(r.program.start),
                            format_time(r.program.stop)
                        ),
                        Style::default().fg(Color::DarkGray),
                    ));
                    spans.push(Span::raw("  "));
                }
                spans.push(Span::styled(
                    &r.program.title,
                    Style::default().fg(if r.is_live {
                        Color::Green
                    } else {
                        Color::White
                    }),
                ));
                ListItem::new(Line::from(spans))
            })
            .collect();

        let title = format!(" Results ({}) ", app.ai.results.len());
        let results_block = Block::default()
            .title(title)
            .title_bottom(Line::from(" Enter: play | D: detail | Tab: chat ").fg(Color::DarkGray))
            .borders(Borders::ALL)
            .border_style(focus_border(app.ai.focus_results));
        let list = List::new(items).block(results_block).highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 0, 40))
                .fg(Color::Magenta)
                .bold(),
        );
        f.render_stateful_widget(list, main_chunks[0], &mut app.nav.ai_state);
    }

    let bottom_chunks = Layout::default()
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(main_chunks[1]);

    let mut chat_lines: Vec<Line> = Vec::new();
    for msg in &app.ai.chat_history {
        if msg.is_user {
            chat_lines.push(Line::from(vec![
                Span::styled("You: ", Style::default().fg(theme).bold()),
                Span::styled(&msg.text, Style::default().fg(Color::White)),
            ]));
        } else {
            for line in msg.text.lines() {
                chat_lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(Color::Rgb(180, 200, 255)),
                )));
            }
        }
        chat_lines.push(Line::from(""));
    }
    if app.ai.loading {
        chat_lines.push(Line::from(Span::styled(
            "  Thinking...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    let inner_w = bottom_chunks[0].width.saturating_sub(2) as usize;
    let visible_h = bottom_chunks[0].height.saturating_sub(2) as usize;
    let mut wrapped_total = 0usize;
    for line in &chat_lines {
        let chars: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        wrapped_total += if inner_w > 0 && chars > inner_w {
            chars.div_ceil(inner_w)
        } else {
            1
        };
    }
    let scroll = if wrapped_total > visible_h {
        (wrapped_total - visible_h) as u16
    } else {
        0
    };

    f.render_widget(
        Paragraph::new(chat_lines)
            .wrap(Wrap { trim: true })
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title(" Chat ")
                    .borders(Borders::ALL)
                    .border_style(focus_border(!app.ai.focus_results)),
            ),
        bottom_chunks[0],
    );

    let input_border = if app.ai.loading { Color::Yellow } else { theme };
    f.render_widget(
        Paragraph::new(format!(" > {}", app.ai.query)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(input_border)),
        ),
        bottom_chunks[1],
    );

    let hint = if app.ai.focus_results {
        " Enter: play | D: details | Tab: chat | ESC: back "
    } else {
        " Enter: send | Tab: results | ESC: back "
    };
    f.render_widget(Paragraph::new(hint).fg(Color::DarkGray), bottom_chunks[2]);
}

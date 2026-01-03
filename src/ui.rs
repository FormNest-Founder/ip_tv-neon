use crate::app::App;
use crate::epg::{find_epg_id, get_current_epg};
use crate::models::Screen;
use chrono::{DateTime, Local, Utc};
use ratatui::{prelude::*, widgets::*};

pub fn ui(f: &mut Frame, app: &mut App) {
    let size = f.size();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" NIGHT CITY HUB ")
        .border_style(Style::default().fg(Color::Rgb(
            app.config.theme_color.0,
            app.config.theme_color.1,
            app.config.theme_color.2,
        )));
    f.render_widget(block.clone(), size);
    let area = block.inner(size);
    match app.screen {
        Screen::Updating => {
            f.render_widget(
                Paragraph::new("\n\n🚀 UPDATING DATA...\nPLEASE WAIT...")
                    .alignment(Alignment::Center)
                    .fg(Color::Yellow)
                    .bold(),
                area,
            );
        }
        Screen::MainMenu => {
            let chunks = Layout::default()
                .constraints([Constraint::Length(10), Constraint::Min(0)])
                .split(area);
            let version = env!("CARGO_PKG_VERSION");
            let status_text = format!(
                "   NEON HUB\n   V {}\n   Channels: {}\n   Radio: {}",
                version,
                app.data.channels.len(),
                app.data.radio.len()
            );

            f.render_widget(
                Paragraph::new(status_text).alignment(Alignment::Center).fg(
                    if app.last_error.is_some() {
                        Color::Red
                    } else {
                        Color::Cyan
                    },
                ),
                chunks[0],
            );
            let items = [
                "📺 IPTV",
                "📻 RADIO",
                "📂 LOCAL",
                "🔗 PLAY LINK",
                "⭐ FAVORITES",
                "🕒 HISTORY",
                "⏹ STOP ALL",
                "🔄 UPDATE",
                "⚙️ SETTINGS",
                "🚪 EXIT",
            ];
            let list = List::new(items.iter().map(|i| ListItem::new(*i)).collect::<Vec<_>>())
                .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black));
            f.render_stateful_widget(list, chunks[1], &mut app.m_state);
        }
        Screen::CatList => {
            let list = List::new(
                app.data
                    .groups
                    .iter()
                    .map(|g| ListItem::new(format!("📂 {}", g)))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(" Categories "))
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(40, 0, 40))
                    .fg(Color::Rgb(255, 0, 255))
                    .add_modifier(Modifier::BOLD),
            );
            f.render_stateful_widget(list, area, &mut app.cat_state);
        }
        Screen::RadioCatList => {
            let list = List::new(
                app.data
                    .radio_groups
                    .iter()
                    .map(|g| ListItem::new(format!("📻 {}", g)))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(" Radio Genres "))
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(40, 40, 0))
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            f.render_stateful_widget(list, area, &mut app.r_cat_state);
        }
        Screen::ChanList => {
            let chunks = Layout::default()
                .constraints([Constraint::Min(0), Constraint::Length(3)])
                .split(area);
            let now = Utc::now().timestamp();
            let items: Vec<ListItem> = if app.filtered.is_empty() {
                vec![ListItem::new("No channels found. Try Update.")]
            } else {
                app.filtered
                    .iter()
                    .map(|&idx| {
                        if idx >= app.data.channels.len() {
                            return ListItem::new("Error");
                        }
                        let ch = &app.data.channels[idx];
                        let mut spans = Vec::new();

                        if let Some(cap) = app.clean_regex.captures(&ch.name) {
                            spans.push(Span::styled(
                                format!("{} ", &cap[1]),
                                Style::default().fg(Color::Cyan).bold(),
                            ));
                            spans.push(Span::styled(
                                &ch.name[cap[0].len()..],
                                Style::default().fg(Color::White),
                            ));
                        } else {
                            spans.push(Span::styled(&ch.name, Style::default().fg(Color::White)));
                        }
                        if let Some(p) = get_current_epg(ch, &app.data, now) {
                            let total = p.stop - p.start;
                            let elapsed = now - p.start;
                            let pct = if total > 0 {
                                (elapsed as f64 / total as f64).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let filled = (pct * 10.0).round() as usize;
                            let bar: String = (0..10)
                                .map(|i| if i < filled { '█' } else { '░' })
                                .collect();
                            spans.push(Span::styled(
                                format!(" │ {} 🔴 {} ", bar, p.title),
                                Style::default().fg(Color::Magenta),
                            ));
                        }
                        ListItem::new(Line::from(spans))
                    })
                    .collect()
            };
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(app.title.as_str())
                        .borders(Borders::ALL),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::Rgb(0, 40, 40))
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_stateful_widget(list, chunks[0], &mut app.ch_state);
            f.render_widget(
                Paragraph::new(format!(" SEARCH: {}", app.search)).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
                chunks[1],
            );
        }
        Screen::RadioList => {
            let cat_idx = app.r_cat_state.selected().unwrap_or(0);
            let category = if app.data.radio_groups.is_empty() {
                "All"
            } else {
                &app.data.radio_groups[cat_idx]
            };

            let filtered_radio: Vec<_> = app
                .data
                .radio
                .iter()
                .filter(|r| {
                    category == "All"
                        || r.genres
                            .iter()
                            .any(|g| g.to_uppercase() == category.to_uppercase())
                })
                .collect();

            let items: Vec<ListItem> = filtered_radio
                .iter()
                .map(|r| {
                    let mut spans = vec![
                        Span::styled(
                            format!("[{}] ", &r.provider[..1]),
                            Style::default().fg(if r.provider == "Record" {
                                Color::Rgb(255, 0, 255)
                            } else {
                                Color::Yellow
                            }),
                        ),
                        Span::styled(
                            format!("{} ", r.title),
                            Style::default().fg(Color::White).bold(),
                        ),
                    ];
                    if let Some(t) = &r.track {
                        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                        spans.push(Span::styled(t.clone(), Style::default().fg(Color::Cyan)));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();

            let title = format!(" Radio: {} (Found: {}) ", category, filtered_radio.len());
            let list = List::new(items)
                .block(Block::default().title(title).borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .bg(Color::Rgb(20, 20, 40))
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_stateful_widget(list, area, &mut app.r_state);
        }
        Screen::LocalList => {
            let items: Vec<ListItem> = app
                .local_files
                .iter()
                .map(|p| {
                    ListItem::new(format!(
                        "📄 {}",
                        p.file_name().unwrap_or_default().to_string_lossy()
                    ))
                })
                .collect();
            let list = List::new(items)
                .block(Block::default().title(" Local Playlists "))
                .highlight_style(Style::default().bg(Color::Rgb(20, 60, 20)));
            f.render_stateful_widget(list, area, &mut app.l_state);
        }
        Screen::Detail => {
            if app.filtered.is_empty() {
                f.render_widget(Paragraph::new("No Data"), area);
                return;
            }
            let idx = app.filtered[app.ch_state.selected().unwrap_or(0)];
            let ch = &app.data.channels[idx];

            let mut items = Vec::new();
            let mut found_epg = false;
            let mut selected_desc = String::from("No description.");
            let mut selected_title = String::from("");

            if let Some(id) = find_epg_id(ch, &app.data) {
                if let Some(progs) = app.data.epg.get(&id) {
                    found_epg = true;
                    let now = Utc::now().timestamp();
                    let sel_idx = app.d_state.selected().unwrap_or(0);

                    for (i, p) in progs.iter().enumerate() {
                        let start_dt = DateTime::<Utc>::from_timestamp(p.start, 0)
                            .unwrap()
                            .with_timezone(&Local);
                        let time_str = start_dt.format("%H:%M").to_string();
                        let (icon, style) = if p.start > now {
                            ("📅", Style::default().fg(Color::DarkGray))
                        } else if p.stop < now {
                            ("⏪", Style::default().fg(Color::Green))
                        } else {
                            ("🔴", Style::default().fg(Color::Magenta).bold())
                        };
                        items.push(ListItem::new(Line::from(vec![
                            Span::styled(format!("{} {} ", icon, time_str), style),
                            Span::styled(&p.title, Style::default().fg(Color::White)),
                        ])));

                        if i == sel_idx {
                            selected_title = p.title.clone();
                            selected_desc = if p.desc.is_empty() {
                                "No description available.".to_string()
                            } else {
                                p.desc.clone()
                            };
                        }
                    }
                }
            }
            if !found_epg {
                items.push(ListItem::new("No EPG Data"));
            }

            let width = area.width.saturating_sub(2).max(1) as usize;
            let desc_lines: u16 = selected_desc
                .lines()
                .map(|l| (l.chars().count() as f64 / width as f64).ceil() as u16)
                .sum::<u16>()
                .max(1);
            let height = (desc_lines + 2).min(area.height / 2).max(3);

            let chunks = Layout::default()
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(height),
                ])
                .split(area);
            let header_spans = vec![Span::styled(
                format!("📺 {}", ch.name),
                Style::default().fg(Color::Cyan).bold(),
            )];
            f.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

            let list = List::new(items)
                .block(Block::default().title(" Schedule ").borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 60)));
            f.render_stateful_widget(list, chunks[1], &mut app.d_state);

            let desc_block = Block::default()
                .title(format!(" Info: {} ", selected_title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));
            f.render_widget(
                Paragraph::new(selected_desc)
                    .block(desc_block)
                    .wrap(Wrap { trim: true }),
                chunks[2],
            );
        }
        Screen::Settings => {
            let items = [
                format!("Playlist: {}", app.config.playlist_url),
                format!("EPG: {}", app.config.epg_url),
                "Save & Exit".into(),
            ];
            let list = List::new(
                items
                    .iter()
                    .map(|i| ListItem::new(i.as_str()))
                    .collect::<Vec<_>>(),
            )
            .highlight_style(Style::default().bg(Color::Yellow).fg(Color::Black));
            f.render_stateful_widget(list, area, &mut app.s_state);
        }
        Screen::Input | Screen::LinkInput => {
            let chunks = Layout::default()
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .split(area);
            f.render_widget(
                Paragraph::new(format!(" Editing {}:", app.in_tgt)).fg(Color::Cyan),
                chunks[0],
            );
            f.render_widget(
                Paragraph::new(app.in_buf.as_str()).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
                chunks[1],
            );
        }
    }
}

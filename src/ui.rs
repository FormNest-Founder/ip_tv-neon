use crate::app::App;
use crate::epg::get_current_epg;
use crate::models::{Screen, Channel};
use chrono::Utc;
use ratatui::{prelude::*, widgets::*};

pub fn get_name_by_url<'a>(url: &'a str, channels: &'a [Channel]) -> &'a str {
    channels.iter()
        .find(|ch| ch.url == url)
        .map(|ch| ch.name.as_str())
        .unwrap_or(url)
}

pub fn ui(f: &mut Frame, app: &mut App) {
    let size = f.size();
    let theme_fg = Color::Rgb(app.config.theme_color.0, app.config.theme_color.1, app.config.theme_color.2);
    let block = Block::default().borders(Borders::ALL).title(" NIGHT CITY HUB ").border_style(Style::default().fg(theme_fg));
    f.render_widget(block.clone(), size);
    let area = block.inner(size);
    match app.screen {
        Screen::Updating => {
            f.render_widget(Paragraph::new("

🚀 UPDATING DATA...
PLEASE WAIT...").alignment(Alignment::Center).fg(Color::Yellow).bold(), area);
        }
        Screen::MainMenu => {
            let chunks = Layout::default().constraints([Constraint::Length(10), Constraint::Min(0)]).split(area);
            let version = env!("CARGO_PKG_VERSION");
            let status_text = format!("   NEON HUB
   V {}
   Channels: {}
   Radio: {}", version, app.data.channels.len(), app.data.radio.len());
            f.render_widget(Paragraph::new(status_text).alignment(Alignment::Center).fg(Color::Cyan), chunks[0]);
            let items = ["📺 IPTV", "📻 RADIO", "📂 LOCAL", "🔗 PLAY LINK", "⭐ FAVORITES", "🕒 HISTORY", "⏹ STOP ALL", "🔄 UPDATE", "⚙️ SETTINGS", "🚪 EXIT"];
            let list = List::new(items.iter().map(|i| ListItem::new(*i)).collect::<Vec<_>>()).highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black));
            f.render_stateful_widget(list, chunks[1], &mut app.m_state);
        }
        Screen::CatList => {
            let items: Vec<ListItem> = app.data.groups.iter().map(|g| {
                let count = app.data.channels.iter().filter(|ch| ch.group == *g).count();
                ListItem::new(format!("📂 {} ({})", g, count))
            }).collect();
            let list = List::new(items)
                .block(Block::default().title(" Categories ").borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::Rgb(40, 0, 40)).fg(Color::Magenta).add_modifier(Modifier::BOLD));
            f.render_stateful_widget(list, area, &mut app.cat_state);
        }
        Screen::ChanList => {
            let chunks = Layout::default().constraints([Constraint::Min(0), Constraint::Length(3)]).split(area);
            let now = Utc::now().timestamp();
            let items: Vec<ListItem> = app.filtered.iter().map(|&idx| {
                let ch = &app.data.channels[idx];
                let mut spans = vec![Span::styled(app.clean_name(&ch.name), Style::default().fg(Color::White))];
                if let Some(p) = get_current_epg(ch, &app.data, now) {
                    let pct = if p.stop > p.start { ((now - p.start) as f64 / (p.stop - p.start) as f64).clamp(0.0, 1.0) } else { 0.0 };
                    let bar: String = (0..10).map(|i| if i < (pct * 10.0) as usize { "█" } else { "░" }).collect();
                    spans.push(Span::styled(format!(" │ {} 🔴 {} ", bar, p.title), Style::default().fg(Color::Magenta)));
                }
                ListItem::new(Line::from(spans))
            }).collect();
            let title = format!(" {} ({}) ", app.selected_group, app.filtered.len());
            f.render_stateful_widget(List::new(items).block(Block::default().title(title).borders(Borders::ALL)).highlight_style(Style::default().bg(Color::Rgb(0, 40, 40)).fg(Color::Cyan).add_modifier(Modifier::BOLD)), chunks[0], &mut app.ch_state);
            f.render_widget(Paragraph::new(format!(" SEARCH: {}", app.search)).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow))), chunks[1]);
        }
        Screen::RadioCatList => {
            let items: Vec<ListItem> = app.data.radio_groups.iter().map(|g| {
                let count = if g == "All" {
                    app.data.radio.len()
                } else {
                    app.data.radio.iter().filter(|r| r.genres.contains(g)).count()
                };
                ListItem::new(format!("🎵 {} ({})", g, count))
            }).collect();
            let list = List::new(items)
                .block(Block::default().title(" Radio Genres ").borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::Rgb(40, 0, 40)).fg(Color::Magenta).add_modifier(Modifier::BOLD));
            f.render_stateful_widget(list, area, &mut app.r_cat_state);
        }
        Screen::RadioList => {
            let items: Vec<ListItem> = app.filtered_radio.iter().map(|&idx| {
                let st = &app.data.radio[idx];
                let track_info = st.track.as_deref().unwrap_or("");
                let mut spans = vec![Span::styled(&st.title, Style::default().fg(Color::White))];
                if !track_info.is_empty() {
                    spans.push(Span::styled(format!(" │ 🎶 {}", track_info), Style::default().fg(Color::Green)));
                }
                ListItem::new(Line::from(spans))
            }).collect();
            let title = format!(" Radio: {} ({}) ", app.selected_radio_genre, app.filtered_radio.len());
            let list = List::new(items)
                .block(Block::default().title(title).borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::Rgb(0, 30, 0)).fg(Color::Green).add_modifier(Modifier::BOLD));
            f.render_stateful_widget(list, area, &mut app.r_state);
        }
        Screen::Favorites => {
            let mut favs: Vec<_> = app.config.favorites.iter().collect();
            favs.sort();
            let items: Vec<ListItem> = favs.iter().map(|url| {
                let name = get_name_by_url(url, &app.data.channels);
                ListItem::new(format!("⭐ {}", app.clean_name(name)))
            }).collect();
            let list = List::new(items)
                .block(Block::default().title(" Favorites ").borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 0)).fg(Color::Yellow).add_modifier(Modifier::BOLD));
            f.render_stateful_widget(list, area, &mut app.fav_state);
        }
        Screen::History => {
            let items: Vec<ListItem> = app.config.history.iter().rev().map(|url| {
                let name = get_name_by_url(url, &app.data.channels);
                ListItem::new(format!("🕒 {}", app.clean_name(name)))
            }).collect();
            let list = List::new(items)
                .block(Block::default().title(" History ").borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::Rgb(0, 0, 40)).fg(Color::Blue).add_modifier(Modifier::BOLD));
            f.render_stateful_widget(list, area, &mut app.hist_state);
        }
        Screen::Settings => {
            let text = format!("Playlist URL: {}
EPG URL: {}
Fullscreen: {}
Geometry: {}

Press ESC to return", 
                app.config.playlist_url, app.config.epg_url, app.config.video_fullscreen, app.config.video_geometry);
            f.render_widget(Paragraph::new(text).block(Block::default().title(" Settings ").borders(Borders::ALL)), area);
        }
        Screen::LocalList => {
            let items: Vec<ListItem> = app.local_files.iter().map(|p| ListItem::new(p.to_string_lossy().to_string())).collect();
            let list = List::new(items)
                .block(Block::default().title(" Local Playlists ").borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::White).fg(Color::Black));
            f.render_stateful_widget(list, area, &mut app.d_state);
        }
        Screen::LinkInput => {
            f.render_widget(Paragraph::new("

Custom Link Input not yet implemented.
Press ESC to return.").alignment(Alignment::Center), area);
        }
        _ => { f.render_widget(Paragraph::new("View not implemented").alignment(Alignment::Center), area); }
    }
}

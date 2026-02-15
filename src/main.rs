use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::time::{Duration, Instant};

mod app;
mod epg;
mod models;
mod ui;
mod utils;
mod consts;

use app::App;
use epg::{update_data, scan_local_playlists};
use models::{Config, Screen};
use ui::ui;
use ratatui::widgets::ListState;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load();
    match std::env::args().nth(1).as_deref() {
        Some("update") => { update_data(&config).await?; return Ok(()); }
        Some("diag") => { return diag(); }
        _ => {}
    }

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, event::EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let mut app = App::new(config);

    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(1000);

    loop {
        if app.needs_redraw || last_tick.elapsed() >= tick_rate {
            terminal.draw(|f| ui(f, &mut app))?;
            app.needs_redraw = false;
            last_tick = Instant::now();
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(&mut app, key).await;
                app.needs_redraw = true;
            }
        }
        if app.quit { break; }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture, event::DisableBracketedPaste)?;
    Ok(())
}

fn nav_up(state: &mut ListState, len: usize) {
    if len == 0 { return; }
    let i = state.selected().unwrap_or(0);
    state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
}

fn nav_down(state: &mut ListState, len: usize) {
    if len == 0 { return; }
    let i = state.selected().unwrap_or(0);
    state.select(Some(if i >= len - 1 { 0 } else { i + 1 }));
}

async fn handle_key_event(app: &mut App, key: event::KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') { app.quit = true; return; }
    match app.screen {
        Screen::MainMenu => match key.code {
            KeyCode::Up => nav_up(&mut app.m_state, 10),
            KeyCode::Down => nav_down(&mut app.m_state, 10),
            KeyCode::Enter => match app.m_state.selected().unwrap_or(0) {
                0 => app.screen = Screen::CatList,
                1 => app.screen = Screen::RadioCatList,
                2 => {
                    app.local_files = scan_local_playlists();
                    app.screen = Screen::LocalList;
                    app.d_state.select(Some(0));
                }
                3 => app.screen = Screen::LinkInput,
                4 => {
                    app.screen = Screen::Favorites;
                    app.fav_state.select(Some(0));
                }
                5 => {
                    app.screen = Screen::History;
                    app.hist_state.select(Some(0));
                }
                6 => app.stop_all(),
                7 => {
                    app.screen = Screen::Updating;
                }
                8 => app.screen = Screen::Settings,
                9 => app.quit = true,
                _ => {}
            }
            KeyCode::Esc => app.quit = true,
            _ => {}
        }
        Screen::Updating => {
            let _ = update_data(&app.config).await;
            app.reload_data();
            app.screen = Screen::MainMenu;
        }
        Screen::CatList => match key.code {
            KeyCode::Up => nav_up(&mut app.cat_state, app.data.groups.len()),
            KeyCode::Down => nav_down(&mut app.cat_state, app.data.groups.len()),
            KeyCode::Enter => {
                if let Some(idx) = app.cat_state.selected() {
                    if idx < app.data.groups.len() {
                        app.selected_group = app.data.groups[idx].clone();
                        app.search.clear();
                        app.update_filter();
                        app.screen = Screen::ChanList;
                    }
                }
            }
            KeyCode::Esc => app.screen = Screen::MainMenu,
            _ => {}
        }
        Screen::ChanList => match key.code {
            KeyCode::Up => nav_up(&mut app.ch_state, app.filtered.len()),
            KeyCode::Down => nav_down(&mut app.ch_state, app.filtered.len()),
            KeyCode::Enter => {
                if let Some(idx) = app.ch_state.selected() {
                    if idx < app.filtered.len() {
                        let (url, name) = {
                            let ch = &app.data.channels[app.filtered[idx]];
                            (ch.url.clone(), ch.name.clone())
                        };
                        app.run_mpv(&url, &name, "", false);
                        // Добавляем в историю (простейшая реализация)
                        if !app.config.history.contains(&url) {
                           app.config.history.push(url);
                           let _ = app.config.save();
                        }
                        app.quit = true;
                    }
                }
            }
            KeyCode::Char('f') => { // Toggle favorite
                if let Some(idx) = app.ch_state.selected() {
                    let url = app.data.channels[app.filtered[idx]].url.clone();
                    if app.config.favorites.contains(&url) {
                        app.config.favorites.remove(&url);
                    } else {
                        app.config.favorites.insert(url);
                    }
                    let _ = app.config.save();
                }
            }
            KeyCode::Char(c) => { app.search.push(c); app.update_filter(); }
            KeyCode::Backspace => { app.search.pop(); app.update_filter(); }
            KeyCode::Esc => { app.search.clear(); app.screen = Screen::CatList; }
            _ => {}
        }
        Screen::RadioCatList => match key.code {
            KeyCode::Up => nav_up(&mut app.r_cat_state, app.data.radio_groups.len()),
            KeyCode::Down => nav_down(&mut app.r_cat_state, app.data.radio_groups.len()),
            KeyCode::Enter => {
                if let Some(idx) = app.r_cat_state.selected() {
                    if idx < app.data.radio_groups.len() {
                        app.selected_radio_genre = app.data.radio_groups[idx].clone();
                        app.update_radio_filter();
                        app.screen = Screen::RadioList;
                    }
                }
            }
            KeyCode::Esc => app.screen = Screen::MainMenu,
            _ => {}
        }
        Screen::RadioList => match key.code {
            KeyCode::Up => nav_up(&mut app.r_state, app.filtered_radio.len()),
            KeyCode::Down => nav_down(&mut app.r_state, app.filtered_radio.len()),
            KeyCode::Enter => {
                if let Some(idx) = app.r_state.selected() {
                    if idx < app.filtered_radio.len() {
                        let station = &app.data.radio[app.filtered_radio[idx]];
                        let title = station.title.clone();
                        let track = station.track.clone().unwrap_or_default();
                        let url = station.stream.clone();
                        app.run_mpv(&url, &title, &track, true);
                        app.quit = true;
                    }
                }
            }
            KeyCode::Esc => app.screen = Screen::RadioCatList,
            _ => {}
        }
        Screen::Favorites => match key.code {
            KeyCode::Up => nav_up(&mut app.fav_state, app.config.favorites.len()),
            KeyCode::Down => nav_down(&mut app.fav_state, app.config.favorites.len()),
            KeyCode::Enter => {
                if let Some(idx) = app.fav_state.selected() {
                    let mut favs: Vec<_> = app.config.favorites.iter().collect();
                    favs.sort();
                    if idx < favs.len() {
                        let url = favs[idx].clone();
                        let name = ui::get_name_by_url(&url, &app.data.channels).to_string();
                        app.run_mpv(&url, &name, "", false);
                        app.quit = true;
                    }
                }
            }
            KeyCode::Esc => app.screen = Screen::MainMenu,
            _ => {}
        }
        Screen::History => match key.code {
            KeyCode::Up => nav_up(&mut app.hist_state, app.config.history.len()),
            KeyCode::Down => nav_down(&mut app.hist_state, app.config.history.len()),
            KeyCode::Enter => {
                if let Some(idx) = app.hist_state.selected() {
                    let history: Vec<_> = app.config.history.iter().rev().collect();
                    if idx < history.len() {
                        let url = history[idx].clone();
                        let name = ui::get_name_by_url(&url, &app.data.channels).to_string();
                        app.run_mpv(&url, &name, "", false);
                        app.quit = true;
                    }
                }
            }
            KeyCode::Esc => app.screen = Screen::MainMenu,
            _ => {}
        }
        _ => { if key.code == KeyCode::Esc { app.screen = Screen::MainMenu; } }
    }
}

fn diag() -> Result<()> {
    use crate::models::CacheContainer;
    use crate::consts::get_data_bin_path;
    let path = get_data_bin_path();
    println!("Cache: {:?}", path);
    let f = std::fs::File::open(&path)?;
    let c: CacheContainer = bincode::deserialize_from(f)?;
    let d = &c.data;
    println!("Version: {}", c.version);
    println!("\n=== GROUPS ({}) ===", d.groups.len());
    for g in &d.groups {
        let cnt = d.channels.iter().filter(|ch| ch.group == *g).count();
        println!("  {:30} -> {} ch", g, cnt);
    }
    println!("\n=== CHANNELS (first 5) ===");
    for ch in d.channels.iter().take(5) {
        println!("  [{}] {} url_len={} tvg_id={:?}", ch.group, ch.name, ch.url.len(), ch.tvg_id);
    }
    println!("\n=== RADIO GENRES ({}) ===", d.radio_groups.len());
    for g in &d.radio_groups { print!("{}, ", g); }
    println!("\n\n=== RADIO (first 5) ===");
    for r in d.radio.iter().take(5) {
        println!("  {} stream_len={} track={:?}", r.title, r.stream.len(), r.track);
    }
    println!("\n=== EPG: {} channel IDs ===", d.epg.len());
    println!("=== name_to_id: {} entries ===", d.name_to_id.len());
    Ok(())
}

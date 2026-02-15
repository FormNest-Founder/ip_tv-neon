use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::ListState, Terminal};
use std::time::{Duration, Instant};

mod app;
mod consts;
mod epg;
mod models;
mod ui;
mod utils;

use app::App;
use epg::{scan_local_playlists, update_data};
use models::{Config, Screen, SETTINGS_COUNT};

const MENU_ITEMS: usize = 10;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load();
    let args: Vec<String> = std::env::args().collect();
    let debug = args.iter().any(|a| a == "--debug");

    match args.get(1).map(|s| s.as_str()) {
        Some("update") => {
            update_data(&config).await?;
            return Ok(());
        }
        Some("diag") => return diag(),
        _ => {}
    }

    if debug {
        utils::main_log("=== NEON IPTV DEBUG START ===");
        utils::main_log(&format!(
            "Config: playlist={} epg={}",
            config.playlist_url, config.epg_url
        ));
    }

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, event::EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let mut app = App::new(config);
    app.debug = debug;

    if debug {
        utils::main_log(&format!(
            "Data loaded: {} channels, {} radio, {} groups",
            app.data.channels.len(),
            app.data.radio.len(),
            app.data.groups.len()
        ));
    }

    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(1000);

    loop {
        // Check if MPV has exited
        if let Some(ref mut child) = app.mpv_handle {
            match child.try_wait() {
                Ok(Some(_)) => {
                    app.mpv_handle = None;
                    app.needs_redraw = true;
                }
                _ => {}
            }
        }

        if app.needs_redraw || last_tick.elapsed() >= tick_rate {
            terminal.draw(|f| ui::ui(f, &mut app))?;
            app.needs_redraw = false;
            last_tick = Instant::now();
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                handle_key(&mut app, key).await;
                app.needs_redraw = true;
            }
        }
        if app.quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        event::DisableBracketedPaste
    )?;
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

async fn handle_key(app: &mut App, key: event::KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.stop_all();
        app.quit = true;
        return;
    }

    // While MPV is playing, ESC stops it and returns to TUI
    if app.mpv_handle.is_some() {
        if key.code == KeyCode::Esc {
            app.stop_all();
        }
        return;
    }

    match &app.screen.clone() {
        Screen::MainMenu => match key.code {
            KeyCode::Up => nav_up(&mut app.m_state, MENU_ITEMS),
            KeyCode::Down => nav_down(&mut app.m_state, MENU_ITEMS),
            KeyCode::Enter => match app.m_state.selected().unwrap_or(0) {
                0 => app.screen = Screen::CatList,
                1 => app.screen = Screen::RadioCatList,
                2 => {
                    app.local_files = scan_local_playlists();
                    app.d_state.select(Some(0));
                    app.screen = Screen::LocalList;
                }
                3 => app.screen = Screen::LinkInput,
                4 => {
                    app.fav_state.select(Some(0));
                    app.screen = Screen::Favorites;
                }
                5 => {
                    app.hist_state.select(Some(0));
                    app.screen = Screen::History;
                }
                6 => app.stop_all(),
                7 => app.screen = Screen::Updating,
                8 => {
                    app.status_msg = None;
                    app.screen = Screen::Settings;
                }
                9 => app.quit = true,
                _ => {}
            },
            KeyCode::Esc => app.quit = true,
            _ => {}
        },

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
        },

        Screen::ChanList => match key.code {
            KeyCode::Up => nav_up(&mut app.ch_state, app.filtered.len()),
            KeyCode::Down => nav_down(&mut app.ch_state, app.filtered.len()),
            KeyCode::Enter => {
                if let Some(idx) = app.ch_state.selected() {
                    if idx < app.filtered.len() {
                        app.open_detail(app.filtered[idx]);
                    }
                }
            }
            KeyCode::Char('f') => {
                if let Some(idx) = app.ch_state.selected() {
                    if idx < app.filtered.len() {
                        let url = app.data.channels[app.filtered[idx]].url.clone();
                        if app.config.favorites.contains(&url) {
                            app.config.favorites.remove(&url);
                        } else {
                            app.config.favorites.insert(url);
                        }
                        let _ = app.config.save();
                    }
                }
            }
            KeyCode::Char(c) => {
                app.search.push(c);
                app.update_filter();
            }
            KeyCode::Backspace => {
                app.search.pop();
                app.update_filter();
            }
            KeyCode::Esc => {
                app.search.clear();
                app.screen = Screen::CatList;
            }
            _ => {}
        },

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
        },

        Screen::RadioList => match key.code {
            KeyCode::Up => nav_up(&mut app.r_state, app.filtered_radio.len()),
            KeyCode::Down => nav_down(&mut app.r_state, app.filtered_radio.len()),
            KeyCode::Enter => {
                if let Some(idx) = app.r_state.selected() {
                    if idx < app.filtered_radio.len() {
                        let st = &app.data.radio[app.filtered_radio[idx]];
                        let title = st.title.clone();
                        let track = st.track.clone().unwrap_or_default();
                        let url = st.stream.clone();
                        app.run_mpv(&url, &title, &track, true);
                    }
                }
            }
            KeyCode::Esc => app.screen = Screen::RadioCatList,
            _ => {}
        },

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
                    }
                }
            }
            KeyCode::Esc => app.screen = Screen::MainMenu,
            _ => {}
        },

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
                    }
                }
            }
            KeyCode::Esc => app.screen = Screen::MainMenu,
            _ => {}
        },

        Screen::Settings => match key.code {
            KeyCode::Up => nav_up(&mut app.set_state, SETTINGS_COUNT),
            KeyCode::Down => nav_down(&mut app.set_state, SETTINGS_COUNT),
            KeyCode::Enter => {
                let idx = app.set_state.selected().unwrap_or(0);
                match idx {
                    0 | 1 | 3 => {
                        app.edit_buf = app.settings_value(idx);
                        app.screen = Screen::SettingsEdit(idx);
                    }
                    2 | 4 | 5 | 6 => {
                        app.settings_toggle(idx);
                    }
                    _ => {}
                }
            }
            KeyCode::Esc => app.screen = Screen::MainMenu,
            _ => {}
        },

        Screen::SettingsEdit(field) => {
            let field = *field;
            match key.code {
                KeyCode::Char(c) => app.edit_buf.push(c),
                KeyCode::Backspace => { app.edit_buf.pop(); }
                KeyCode::Enter => {
                    let val = app.edit_buf.clone();
                    app.settings_apply(field, &val);
                    app.status_msg = Some("Saved".into());
                    app.screen = Screen::Settings;
                }
                KeyCode::Esc => {
                    app.edit_buf.clear();
                    app.screen = Screen::Settings;
                }
                _ => {}
            }
        }

        Screen::Detail => match key.code {
            KeyCode::Up => nav_up(&mut app.epg_state, app.detail_programs.len()),
            KeyCode::Down => nav_down(&mut app.epg_state, app.detail_programs.len()),
            KeyCode::Enter => app.detail_play_selected(),
            KeyCode::Char('l') => app.detail_play_live(),
            KeyCode::Char('f') => {
                if let Some(ch_idx) = app.detail_channel {
                    let url = app.data.channels[ch_idx].url.clone();
                    if app.config.favorites.contains(&url) {
                        app.config.favorites.remove(&url);
                    } else {
                        app.config.favorites.insert(url);
                    }
                    let _ = app.config.save();
                }
            }
            KeyCode::Esc => app.screen = Screen::ChanList,
            _ => {}
        },

        Screen::LocalList | Screen::LinkInput => {
            if key.code == KeyCode::Esc {
                app.screen = Screen::MainMenu;
            }
        }
    }
}

fn diag() -> Result<()> {
    use consts::get_data_bin_path;
    use models::CacheContainer;

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
        println!(
            "  [{}] {} url_len={} tvg_id={:?}",
            ch.group,
            ch.name,
            ch.url.len(),
            ch.tvg_id
        );
    }
    println!("\n=== RADIO GENRES ({}) ===", d.radio_groups.len());
    for g in &d.radio_groups {
        print!("{}, ", g);
    }
    println!("\n\n=== RADIO (first 5) ===");
    for r in d.radio.iter().take(5) {
        println!(
            "  {} stream_len={} track={:?}",
            r.title,
            r.stream.len(),
            r.track
        );
    }
    println!("\n=== EPG: {} channel IDs ===", d.epg.len());
    println!("=== name_to_id: {} entries ===", d.name_to_id.len());
    Ok(())
}

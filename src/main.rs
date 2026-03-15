// ─── Imports ─────────────────────────────────────────────────────────────────

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::ListState, Terminal};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ─── Modules ─────────────────────────────────────────────────────────────────

mod ai;
mod app;
mod consts;
mod epg;
mod models;
mod ui;
mod utils;

use app::App;
use epg::{fetch_radio_now, scan_local_playlists, update_data};
use models::{Config, Screen, SETTINGS_COUNT};

// ─── Constants ───────────────────────────────────────────────────────────────

const MENU_ITEMS: usize = 10;

// ─── Entry Point & Event Loop ────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load();
    let args: Vec<String> = std::env::args().collect();
    let debug = args.iter().any(|a| a == "--debug");

    let http_client = reqwest::Client::builder()
        .user_agent(consts::UA)
        .timeout(Duration::from_secs(15))
        .build()?;

    match args.get(1).map(|s| s.as_str()) {
        Some("update") => {
            update_data(&config, &http_client).await?;
            return Ok(());
        }
        Some("diag") => return diag(),
        _ => {}
    }

    // Fix 3: panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, event::EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let mut app = App::new(config);
    app.debug = debug;
    let mut last_tick = Instant::now();
    let mut radio_tracks_dirty = true;
    let mut radio_task: Option<tokio::task::JoinHandle<HashMap<String, String>>> = None;
    let mut ai_task: Option<tokio::task::JoinHandle<ai::AiChatResponse>> = None;
    let mut update_task: Option<tokio::task::JoinHandle<Result<()>>> = None;
    let tick_rate = Duration::from_millis(1000);

    loop {
        // Video suspended: hide TUI, wait for mpv, restore TUI
        if app.suspended {
            if let Some(mut child) = app.mpv_handle.take() {
                let wait_result = tokio::time::timeout(
                    Duration::from_secs(43200), // 12 hours max
                    child.wait()
                ).await;
                if wait_result.is_err() {
                    let _ = child.start_kill();
                }
                app.suspended = false;
                // Restore TUI window via niri
                let _ = std::process::Command::new("niri")
                    .args(["msg", "action", "move-column-to-workspace", "1"])
                    .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
                terminal.clear()?;
                app.needs_redraw = true;
                continue;
            }
        }

        // Check if background MPV (radio) has exited
        if let Some(ref mut child) = app.mpv_handle {
            match child.try_wait() {
                Ok(Some(_)) => {
                    app.mpv_handle = None;
                    app.needs_redraw = true;
                }
                _ => {}
            }
        }

        // Async radio track fetch (non-blocking)
        if matches!(app.screen, Screen::RadioCatList | Screen::RadioList) && radio_tracks_dirty && radio_task.is_none() {
            radio_tracks_dirty = false;
            let client = http_client.clone();
            radio_task = Some(tokio::spawn(async move {
                fetch_radio_now(&client).await
            }));
        }
        let radio_done = radio_task.as_ref().map_or(false, |t| t.is_finished());
        if radio_done {
            if let Some(task) = radio_task.take() {
                if let Ok(tracks) = task.await {
                    for st in &mut app.data.radio {
                        if let Some(t) = tracks.get(&st.id) {
                            st.track = Some(t.clone());
                        }
                    }
                    app.needs_redraw = true;
                }
            }
        }
        if !matches!(app.screen, Screen::RadioCatList | Screen::RadioList) {
            radio_tracks_dirty = true;
        }

        // Check if AI chat task finished
        if ai_task.as_ref().is_some_and(|t| t.is_finished()) {
            if let Some(task) = ai_task.take() {
                if let Ok(response) = task.await {
                    // Add assistant message to chat history
                    app.ai_chat_history.push(ai::ChatMsg {
                        is_user: false,
                        text: response.text,
                    });
                    // If keywords returned — search EPG
                    if let Some(ref kw) = response.keywords {
                        let now = chrono::Utc::now().timestamp();
                        app.ai_results = ai::search_epg(&app.data, kw, now);
                        app.ai_state.select(if app.ai_results.is_empty() { None } else { Some(0) });
                        if !app.ai_results.is_empty() {
                            app.ai_focus_results = true;
                        }
                    }
                    app.ai_loading = false;
                    app.ai_chat_scroll = 0;
                    app.needs_redraw = true;
                }
            }
        }

        // Launch data update as background task (non-blocking)
        if matches!(app.screen, Screen::Updating) && update_task.is_none() {
            let config = app.config.clone();
            let client = http_client.clone();
            update_task = Some(tokio::spawn(async move {
                update_data(&config, &client).await
            }));
        }
        // Check if update task finished
        if update_task.as_ref().is_some_and(|t| t.is_finished()) {
            if let Some(task) = update_task.take() {
                match task.await {
                    Ok(Ok(())) => {
                        app.reload_data();
                        let ch = app.data.channels.len();
                        let rd = app.data.radio.len();
                        let epg = app.data.epg.len();
                        app.status_msg = Some(format!("Updated: {} ch, {} radio, {} EPG", ch, rd, epg));
                    }
                    Ok(Err(e)) => {
                        app.status_msg = Some(format!("Update failed: {}", e));
                    }
                    Err(e) => {
                        app.status_msg = Some(format!("Update task panic: {}", e));
                    }
                }
                app.screen = Screen::MainMenu;
                app.needs_redraw = true;
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
                // AI chat needs http_client and ai_task — handle inline
                let handled = match app.screen {
                    Screen::AiChat => {
                        if app.ai_focus_results {
                            // Focus on results list — typing switches to chat
                            match key.code {
                                KeyCode::Up => nav_up(&mut app.ai_state, app.ai_results.len()),
                                KeyCode::Down => nav_down(&mut app.ai_state, app.ai_results.len()),
                                KeyCode::Enter => app.ai_play_selected(),
                                KeyCode::Char('d') => {
                                    if let Some(idx) = app.ai_state.selected() {
                                        if idx < app.ai_results.len() {
                                            app.open_detail(app.ai_results[idx].channel_idx);
                                        }
                                    }
                                }
                                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    app.ai_focus_results = false;
                                    app.ai_query.push(c);
                                }
                                KeyCode::Backspace => { app.ai_focus_results = false; }
                                KeyCode::Tab => { app.ai_focus_results = false; }
                                KeyCode::Esc => { app.ai_loading = false; app.screen = Screen::MainMenu; }
                                _ => {}
                            }
                        } else {
                            // Focus on chat input
                            match key.code {
                                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => { app.ai_query.push(c); }
                                KeyCode::Backspace => { app.ai_query.pop(); }
                                KeyCode::Enter => {
                                    if !app.ai_query.is_empty() && !app.ai_loading {
                                        let msg = app.ai_query.drain(..).collect::<String>();
                                        app.ai_chat_history.push(ai::ChatMsg {
                                            is_user: true,
                                            text: msg.clone(),
                                        });
                                        app.ai_loading = true;
                                        app.ai_chat_scroll = 0;
                                        let client = http_client.clone();
                                        let history = app.ai_chat_history.clone();
                                        let context = ai::build_context(&app.data, &app.config.history, &app.data.channels);
                                        ai_task = Some(tokio::spawn(async move {
                                            ai::ai_chat(&client, &history[..history.len()-1], &msg, &context).await
                                        }));
                                    }
                                }
                                KeyCode::Tab => {
                                    if !app.ai_results.is_empty() {
                                        app.ai_focus_results = true;
                                    }
                                }
                                KeyCode::Esc => { app.ai_loading = false; app.screen = Screen::MainMenu; }
                                _ => {}
                            }
                        }
                        true
                    }
                    _ => false,
                };
                if !handled {
                    handle_key(&mut app, key).await;
                }
                app.needs_redraw = true;
            }
        }
        if app.quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture, event::DisableBracketedPaste)?;
    Ok(())
}

// ─── Navigation Helpers ──────────────────────────────────────────────────────

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

// ─── Key Handlers ────────────────────────────────────────────────────────────

async fn handle_key(app: &mut App, key: event::KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.stop_all();
        app.quit = true;
        return;
    }

    if app.mpv_handle.is_some() {
        if key.code == KeyCode::Esc {
            app.stop_all();
        }
        return;
    }

    match app.screen {
        Screen::MainMenu => match key.code {
            KeyCode::Up => { app.status_msg = None; nav_up(&mut app.m_state, MENU_ITEMS); }
            KeyCode::Down => { app.status_msg = None; nav_down(&mut app.m_state, MENU_ITEMS); }
            KeyCode::Enter => match app.m_state.selected().unwrap_or(0) {
                0 => app.screen = Screen::CatList,
                1 => app.screen = Screen::RadioCatList,
                2 => {
                    app.local_files = scan_local_playlists();
                    app.d_state.select(Some(0));
                    app.screen = Screen::LocalList;
                }
                3 => {
                    app.ai_query.clear();
                    app.ai_results.clear();
                    app.ai_chat_history.clear();
                    app.ai_focus_results = false;
                    app.ai_chat_scroll = 0;
                    app.screen = Screen::AiChat;
                }
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

        Screen::Updating => {}

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
            KeyCode::Char(c) => { app.search.push(c); app.update_filter(); }
            KeyCode::Backspace => { app.search.pop(); app.update_filter(); }
            KeyCode::Esc => { app.search.clear(); app.screen = Screen::CatList; }
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
                        let (url, title, track) = {
                            let st = &app.data.radio[app.filtered_radio[idx]];
                            (st.stream.clone(), st.title.clone(), st.track.clone().unwrap_or_default())
                        };
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
                    let favs = app.sorted_favorites();
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
                    0 | 1 | 3 => { app.edit_buf = app.settings_value(idx); app.screen = Screen::SettingsEdit(idx); }
                    2 | 4 | 5 | 6 => { app.settings_toggle(idx); }
                    _ => {}
                }
            }
            KeyCode::Esc => app.screen = Screen::MainMenu,
            _ => {}
        },

        Screen::SettingsEdit(field) => {
            match key.code {
                KeyCode::Char(c) => app.edit_buf.push(c),
                KeyCode::Backspace => { app.edit_buf.pop(); }
                KeyCode::Enter => {
                    let val = app.edit_buf.clone();
                    app.settings_apply(field, &val);
                    app.status_msg = Some("Saved".into());
                    app.screen = Screen::Settings;
                }
                KeyCode::Esc => { app.edit_buf.clear(); app.screen = Screen::Settings; }
                _ => {}
            }
        }

        Screen::Detail => match key.code {
            KeyCode::Up => nav_up(&mut app.epg_state, app.detail_programs.len()),
            KeyCode::Down => nav_down(&mut app.epg_state, app.detail_programs.len()),
            KeyCode::Enter => app.detail_play_selected(),
            KeyCode::Char('l') => app.detail_play_live(),
            KeyCode::Char('f') => {
                if let Some(ch_idx) = app.detail_channel.filter(|&i| i < app.data.channels.len()) {
                    let url = app.data.channels[ch_idx].url.clone();
                    if app.config.favorites.contains(&url) { app.config.favorites.remove(&url); }
                    else { app.config.favorites.insert(url); }
                    let _ = app.config.save();
                }
            }
            KeyCode::Esc => {
                app.screen = app.detail_return_screen.take().unwrap_or(Screen::ChanList);
            }
            _ => {}
        },

        Screen::AiChat => {} // Handled in main loop

        Screen::LocalList | Screen::LinkInput => {
            if key.code == KeyCode::Esc { app.screen = Screen::MainMenu; }
        }
    }
}

// ─── Diagnostics ─────────────────────────────────────────────────────────────

fn diag() -> Result<()> {
    println!("NEON IPTV Diagnostics");
    println!("Config: {}", consts::get_config_json_path().display());
    println!("Cache:  {}", consts::get_data_bin_path().display());
    println!("Config exists: {}", consts::get_config_json_path().exists());
    println!("Cache exists:  {}", consts::get_data_bin_path().exists());
    if let Ok(raw) = std::fs::read_to_string(consts::get_config_json_path()) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            println!("Playlist: {}", v["playlist_url"].as_str().unwrap_or("N/A"));
            println!("EPG URL:  {}", v["epg_url"].as_str().unwrap_or("N/A"));
        }
    }
    Ok(())
}

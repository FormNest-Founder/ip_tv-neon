// ─── Imports ─────────────────────────────────────────────────────────────────

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::ListState, Terminal};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::time::interval;

// ─── Modules ─────────────────────────────────────────────────────────────────

mod ai;
mod app;
mod consts;
mod epg;
mod models;
mod mpv_ipc;
mod player;
mod ui;
mod utils;

use app::App;
use epg::{fetch_radio_now, scan_local_playlists, update_data};
use models::{Config, Screen, SETTINGS_COUNT};

// ─── Constants ───────────────────────────────────────────────────────────────

const MENU_ITEMS: usize = 10;
/// Normal UI redraw rate (keyboard-driven)
const TICK_RATE: Duration = Duration::from_millis(1000);
/// Radio animation tick: 20 FPS — drives VU meters and marquee
const RADIO_TICK: Duration = Duration::from_millis(50);

// ─── Entry Point & Event Loop ────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load();
    let args: Vec<String> = std::env::args().collect();
    let debug = args.iter().any(|a| a == "--debug");

    // No total request timeout: the EPG body may be up to 128 MB and a total
    // timeout (incl. body read) would abort a legitimate large download. Bound
    // the connect phase and the idle-between-reads instead. AI calls set their
    // own per-request .timeout(30s).
    let http_client = reqwest::Client::builder()
        .user_agent(consts::UA)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()?;

    match args.get(1).map(|s| s.as_str()) {
        Some("update") => {
            update_data(&config, &http_client).await?;
            return Ok(());
        }
        Some("diag") => return diag(),
        _ => {}
    }

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            event::DisableBracketedPaste
        );
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    // No EnableMouseCapture: there is no mouse handling, and capturing the mouse
    // breaks the terminal's native text selection/copy.
    execute!(stdout, EnterAlternateScreen, event::EnableBracketedPaste)?;
    // RAII guard: restores the terminal on ANY exit path — normal return, an
    // early `?` error, or a panic that unwinds past here.
    let _guard = TerminalGuard;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let mut app = App::new(config);
    app.debug = debug;
    let mut last_tick = Instant::now();
    let mut radio_tracks_dirty = true;
    let mut radio_task: Option<tokio::task::JoinHandle<HashMap<String, String>>> = None;
    let mut ai_task: Option<tokio::task::JoinHandle<ai::AiChatResponse>> = None;
    let mut update_task: Option<tokio::task::JoinHandle<Result<()>>> = None;

    // 50ms interval for radio animation — only polled when radio IPC is active
    let mut radio_interval = interval(RADIO_TICK);
    radio_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        // ── Radio animation tick ──────────────────────────────────────────────
        // When radio is playing, the loop needs to tick at ~20 FPS for VU meters
        // and marquee. The previous `select! { _ = sleep(Duration::ZERO) => {} }`
        // fallback caused a CPU spinloop. Now we simply await the interval or
        // keyboard input, whichever arrives first — truly non-blocking and zero
        // CPU waste when idle.
        if app.player.radio_ipc.is_some() {
            tokio::select! {
                biased;
                _ = radio_interval.tick() => {
                    app.tick_radio();
                    app.needs_redraw = true;
                }
                // Yield once to let the rest of the loop body run even when the
                // interval is not yet ready. tokio::task::yield_now() returns
                // Pending exactly once, then Ready — no spinloop.
                _ = tokio::task::yield_now() => {}
            }
        }

        handle_mpv_exit(&mut app);
        handle_radio_fetch(&mut app, &http_client, &mut radio_tracks_dirty, &mut radio_task).await;
        handle_ai_task(&mut app, &mut ai_task).await;
        handle_update_task(&mut app, &http_client, &mut update_task).await;

        // ── Draw ──────────────────────────────────────────────────────────────
        if app.needs_redraw || last_tick.elapsed() >= TICK_RATE {
            terminal.draw(|f| ui::ui(f, &mut app))?;
            app.needs_redraw = false;
            last_tick = Instant::now();
        }

        // ── Keyboard events ───────────────────────────────────────────────────
        let poll_timeout = if app.player.radio_ipc.is_some() {
            Duration::from_millis(16)
        } else {
            TICK_RATE.saturating_sub(last_tick.elapsed())
        };

        if event::poll(poll_timeout)? {
            handle_input_event(&mut app, event::read()?, &mut ai_task, &http_client).await;
        }
        if app.quit {
            break;
        }
    }

    // Terminal restoration is handled by `_guard` (TerminalGuard::drop).
    Ok(())
}

/// Checks if the background MPV player has exited and cleans up IPC state.
fn handle_mpv_exit(app: &mut App) {
    if let Some(ref mut child) = app.player.mpv_handle {
        if let Ok(Some(_)) = child.try_wait() {
            app.player.mpv_handle = None;
            app.player.radio_ipc = None;
            *app.player.radio_state
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = mpv_ipc::RadioState::default();
            app.player.radio_station_title.clear();
            app.needs_redraw = true;
        }
    }
}

/// Spawns and monitors the radio track fetch background task.
async fn handle_radio_fetch(
    app: &mut App,
    http_client: &reqwest::Client,
    radio_tracks_dirty: &mut bool,
    radio_task: &mut Option<tokio::task::JoinHandle<HashMap<String, String>>>,
) {
    if matches!(app.screen, Screen::RadioCatList | Screen::RadioList)
        && *radio_tracks_dirty
        && radio_task.is_none()
    {
        *radio_tracks_dirty = false;
        let client = http_client.clone();
        *radio_task = Some(tokio::spawn(async move { fetch_radio_now(&client).await }));
    }
    if radio_task.as_ref().is_some_and(|t| t.is_finished()) {
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
        *radio_tracks_dirty = true;
    }
}

/// Processes AI background task completion, pushing results to the chat history.
async fn handle_ai_task(app: &mut App, ai_task: &mut Option<tokio::task::JoinHandle<ai::AiChatResponse>>) {
    if ai_task.as_ref().is_some_and(|t| t.is_finished()) {
        if let Some(task) = ai_task.take() {
            match task.await {
                Ok(response) => {
                    app.ai.chat_history.push(ai::ChatMsg {
                        is_user: false,
                        text: response.text,
                    });
                    // Cap chat history to prevent unbounded memory growth
                    const MAX_CHAT_HISTORY: usize = 100;
                    if app.ai.chat_history.len() > MAX_CHAT_HISTORY {
                        app.ai.chat_history.drain(0..app.ai.chat_history.len() - MAX_CHAT_HISTORY);
                    }
                    if let Some(ref kw) = response.keywords {
                        let now = chrono::Utc::now().timestamp();
                        app.ai.results = ai::search_epg(&app.data, kw, now);
                        app.nav.ai_state.select(if app.ai.results.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
                        if !app.ai.results.is_empty() {
                            app.ai.focus_results = true;
                        }
                    }
                    app.ai.loading = false;
                    app.ai.chat_scroll = 0;
                    app.needs_redraw = true;
                }
                Err(e) => {
                    app.ai.loading = false;
                    app.status_msg = Some(format!("AI task failed: {}", e));
                    app.needs_redraw = true;
                }
            }
        }
    }
}

/// Spawns and processes the background EPG and data update task.
async fn handle_update_task(
    app: &mut App,
    http_client: &reqwest::Client,
    update_task: &mut Option<tokio::task::JoinHandle<Result<()>>>,
) {
    if matches!(app.screen, Screen::Updating) && update_task.is_none() {
        let config = app.config.clone();
        let client = http_client.clone();
        *update_task = Some(tokio::spawn(
            async move { update_data(&config, &client).await },
        ));
    }
    if update_task.as_ref().is_some_and(|t| t.is_finished()) {
        if let Some(task) = update_task.take() {
            match task.await {
                Ok(Ok(())) => {
                    app.reload_data();
                    let ch = app.data.channels.len();
                    let rd = app.data.radio.len();
                    let epg = app.data.epg.len();
                    app.status_msg =
                        Some(format!("Updated: {} ch, {} radio, {} EPG", ch, rd, epg));
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
}

/// Processes input events (keyboard, mouse, paste) and routes them to active screen.
async fn handle_input_event(
    app: &mut App,
    event: Event,
    ai_task: &mut Option<tokio::task::JoinHandle<ai::AiChatResponse>>,
    http_client: &reqwest::Client,
) {
    match event {
        Event::Key(key) => {
            let handled = match app.screen {
                Screen::AiChat => {
                    if app.ai.focus_results {
                        match key.code {
                            KeyCode::Up => nav_up(&mut app.nav.ai_state, app.ai.results.len()),
                            KeyCode::Down => {
                                nav_down(&mut app.nav.ai_state, app.ai.results.len())
                            }
                            KeyCode::Enter => app.ai_play_selected(),
                            KeyCode::Char('d') => {
                                if let Some(idx) = app.nav.ai_state.selected() {
                                    if idx < app.ai.results.len() {
                                        app.open_detail(app.ai.results[idx].channel_idx);
                                    }
                                }
                            }
                            KeyCode::Char(c)
                                if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                app.ai.focus_results = false;
                                app.ai.query.push(c);
                            }
                            KeyCode::Backspace => {
                                app.ai.focus_results = false;
                            }
                            KeyCode::Tab => {
                                app.ai.focus_results = false;
                            }
                            KeyCode::Esc => {
                                app.ai.loading = false;
                                app.screen = Screen::MainMenu;
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char(c)
                                if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                app.ai.query.push(c);
                            }
                            KeyCode::Backspace => {
                                app.ai.query.pop();
                            }
                            KeyCode::Enter
                                if !app.ai.query.is_empty()
                                    && !app.ai.loading
                                    && ai_task.is_none() =>
                            {
                                let msg = app.ai.query.drain(..).collect::<String>();
                                app.ai.chat_history.push(ai::ChatMsg {
                                    is_user: true,
                                    text: msg.clone(),
                                });
                                app.ai.loading = true;
                                app.ai.chat_scroll = 0;
                                let client = http_client.clone();
                                let history = app.ai.chat_history.clone();
                                let context = ai::build_context(
                                    &app.data,
                                    &app.config.history,
                                    &app.data.channels,
                                );
                                let choice =
                                    ai::resolve_choice(&app.config.llm_provider);
                                *ai_task = Some(tokio::spawn(async move {
                                    ai::ai_chat(
                                        &client,
                                        &history[..history.len() - 1],
                                        &msg,
                                        &context,
                                        choice,
                                    )
                                    .await
                                }));
                            }
                            KeyCode::Tab if !app.ai.results.is_empty() => {
                                app.ai.focus_results = true;
                            }
                            KeyCode::Esc => {
                                app.ai.loading = false;
                                app.screen = Screen::MainMenu;
                            }
                            _ => {}
                        }
                    }
                    true
                }
                _ => false,
            };
            if !handled {
                handle_key(app, key).await;
            }
        }
        Event::Paste(data) => handle_paste(app, &data),
        _ => {}
    }
    app.needs_redraw = true;
}

// ─── Terminal Restore Guard ──────────────────────────────────────────────────

/// Restores the terminal to a sane state when dropped: leaves raw mode and the
/// alternate screen and disables mouse capture / bracketed paste. Runs on every
/// exit path (normal, early `?` error, or panic unwind), so the user is never
/// left with a broken terminal.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            event::DisableBracketedPaste
        );
    }
}

// ─── Navigation Helpers ──────────────────────────────────────────────────────

fn nav_up(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let i = state.selected().unwrap_or(0);
    state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
}

fn nav_down(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let i = state.selected().unwrap_or(0);
    state.select(Some(if i >= len - 1 { 0 } else { i + 1 }));
}

// ─── Paste Handling ──────────────────────────────────────────────────────────

/// Route bracketed-paste text into whichever text buffer is currently active.
/// Control characters (incl. newlines/ESC) are stripped so a pasted payload can
/// neither break the single-line inputs nor smuggle terminal escapes.
fn handle_paste(app: &mut App, data: &str) {
    let clean: String = data.chars().filter(|c| !c.is_control()).collect();
    if clean.is_empty() {
        return;
    }
    match app.screen {
        Screen::AiChat => {
            app.ai.focus_results = false;
            app.ai.query.push_str(&clean);
        }
        Screen::ChanList => {
            app.nav.search.push_str(&clean);
            app.update_filter();
        }
        Screen::SettingsEdit(_) => app.nav.edit_buf.push_str(&clean),
        _ => {}
    }
}

// ─── Key Handlers ────────────────────────────────────────────────────────────

async fn handle_key(app: &mut App, key: event::KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.stop_all();
        app.quit = true;
        return;
    }

    // Radio IPC controls — intercept only radio-specific keys.
    // Up/Down are NOT consumed here — they fall through to normal screen handlers
    // so the station list remains navigable while radio plays.
    // Skip interception entirely in text-entry screens (channel search / settings
    // edit) so Space/+/-/m/Esc reach the text buffer instead of the player.
    let in_text_entry = matches!(app.screen, Screen::ChanList | Screen::SettingsEdit(_));
    if app.player.radio_ipc.is_some() && !in_text_entry {
        let cur_vol = app
            .player.radio_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .volume;
        match key.code {
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if let Some(ref ipc) = app.player.radio_ipc {
                    ipc.set_volume((cur_vol + 5.0).min(100.0));
                }
                return;
            }
            KeyCode::Char('-') => {
                if let Some(ref ipc) = app.player.radio_ipc {
                    ipc.set_volume((cur_vol - 5.0).max(0.0));
                }
                return;
            }
            KeyCode::Char(' ') => {
                if let Some(ref ipc) = app.player.radio_ipc {
                    ipc.toggle_pause();
                }
                return;
            }
            KeyCode::Char('m') => {
                let muted = app
                    .player.radio_state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .muted;
                if let Some(ref ipc) = app.player.radio_ipc {
                    ipc.set_mute(!muted);
                }
                return;
            }
            KeyCode::Esc => {
                app.stop_all();
                return;
            }
            _ => {} // All other keys (including ↑↓ Enter) fall through to screen handlers
        }
    }

    // TV/other mpv running
    if app.player.mpv_handle.is_some() {
        match key.code {
            KeyCode::Esc => {
                app.stop_all();
            }
            KeyCode::Up => switch_playing_channel(app, true),
            KeyCode::Down => switch_playing_channel(app, false),
            // When TV is playing, we want to allow volume controls as well (unless it's Radio where it's handled above)
            KeyCode::Char('+') | KeyCode::Char('=') if app.player.radio_ipc.is_none() => {
                // TV volume not strictly handled via IPC right now, but we ignore so it doesn't crash
            }
            _ => {}
        }
        return;
    }

    let screen = app.screen;
    match screen {
        Screen::MainMenu => handle_main_menu_input(app, key.code).await,
        Screen::Updating => {}
        Screen::CatList => handle_cat_list_input(app, key.code),
        Screen::ChanList => handle_chan_list_input(app, key.code),
        Screen::RadioCatList => handle_radio_cat_list_input(app, key.code),
        Screen::RadioList => handle_radio_list_input(app, key.code),
        Screen::Favorites => handle_favorites_input(app, key.code),
        Screen::History => handle_history_input(app, key.code),
        Screen::Settings => handle_settings_input(app, key.code),
        Screen::SettingsEdit(field) => handle_settings_edit_input(app, key.code, field),
        Screen::Detail => handle_detail_input(app, key.code),
        Screen::AiChat => {} // Handled in main loop
        Screen::LocalList | Screen::LinkInput => {
            if key.code == KeyCode::Esc {
                app.screen = Screen::MainMenu;
            }
        }
    }
}

async fn handle_main_menu_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => {
            app.status_msg = None;
            nav_up(&mut app.nav.m_state, MENU_ITEMS);
        }
        KeyCode::Down => {
            app.status_msg = None;
            nav_down(&mut app.nav.m_state, MENU_ITEMS);
        }
        KeyCode::Enter => match app.nav.m_state.selected().unwrap_or(0) {
            0 => app.screen = Screen::CatList,
            1 => app.screen = Screen::RadioCatList,
            2 => {
                let local_dir = app.config.local_dir.clone();
                let files =
                    tokio::task::spawn_blocking(move || scan_local_playlists(&local_dir))
                        .await
                        .unwrap_or_default();
                app.local_files = files;
                app.nav.d_state.select(Some(0));
                app.screen = Screen::LocalList;
            }
            3 => {
                app.ai.query.clear();
                app.ai.results.clear();
                app.ai.chat_history.clear();
                app.ai.focus_results = false;
                app.ai.chat_scroll = 0;
                app.screen = Screen::AiChat;
            }
            4 => {
                app.nav.fav_state.select(Some(0));
                app.screen = Screen::Favorites;
            }
            5 => {
                app.nav.hist_state.select(Some(0));
                app.screen = Screen::History;
            }
            6 => app.stop_all(),
            7 => app.screen = Screen::Updating,
            8 => {
                app.status_msg = None;
                app.screen = Screen::Settings;
            }
            9 => {
                app.stop_all();
                app.quit = true;
            }
            _ => {}
        },
        KeyCode::Esc => {
            app.stop_all();
            app.quit = true;
        }
        _ => {}
    }
}

fn handle_cat_list_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => nav_up(&mut app.nav.cat_state, app.data.groups.len()),
        KeyCode::Down => nav_down(&mut app.nav.cat_state, app.data.groups.len()),
        KeyCode::Enter => {
            if let Some(idx) = app.nav.cat_state.selected() {
                if idx < app.data.groups.len() {
                    app.nav.selected_group = app.data.groups[idx].clone();
                    app.nav.search.clear();
                    app.update_filter();
                    app.screen = Screen::ChanList;
                }
            }
        }
        KeyCode::Esc => app.screen = Screen::MainMenu,
        _ => {}
    }
}

fn handle_chan_list_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => nav_up(&mut app.nav.ch_state, app.nav.filtered.len()),
        KeyCode::Down => nav_down(&mut app.nav.ch_state, app.nav.filtered.len()),
        KeyCode::Enter => {
            if let Some(idx) = app.nav.ch_state.selected() {
                if idx < app.nav.filtered.len() {
                    app.open_detail(app.nav.filtered[idx]);
                }
            }
        }
        // Favorite toggle: uppercase 'F' always, lowercase 'f' only when the
        // search box is empty — otherwise 'f' must reach the search text buffer.
        KeyCode::Char(c @ ('f' | 'F')) if c == 'F' || app.nav.search.is_empty() => {
            if let Some(idx) = app.nav.ch_state.selected() {
                if idx < app.nav.filtered.len() {
                    let ch = &app.data.channels[app.nav.filtered[idx]];
                    let url = ch.url.clone();
                    let name = ch.name.clone();
                    if app.config.favorites.contains(&url) {
                        app.config.favorite_remove(&url);
                    } else {
                        app.config.favorite_add(&url, &name);
                    }
                }
            }
        }
        KeyCode::Char(c) => {
            app.nav.search.push(c);
            app.update_filter();
        }
        KeyCode::Backspace => {
            app.nav.search.pop();
            app.update_filter();
        }
        KeyCode::Esc => {
            app.nav.search.clear();
            app.screen = Screen::CatList;
        }
        _ => {}
    }
}

fn handle_radio_cat_list_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => nav_up(&mut app.nav.r_cat_state, app.data.radio_groups.len()),
        KeyCode::Down => nav_down(&mut app.nav.r_cat_state, app.data.radio_groups.len()),
        KeyCode::Enter => {
            if let Some(idx) = app.nav.r_cat_state.selected() {
                if idx < app.data.radio_groups.len() {
                    app.nav.selected_radio_genre = app.data.radio_groups[idx].clone();
                    app.update_radio_filter();
                    app.screen = Screen::RadioList;
                }
            }
        }
        KeyCode::Esc => app.screen = Screen::MainMenu,
        _ => {}
    }
}

fn handle_radio_list_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => nav_up(&mut app.nav.r_state, app.nav.filtered_radio.len()),
        KeyCode::Down => nav_down(&mut app.nav.r_state, app.nav.filtered_radio.len()),
        KeyCode::Enter => {
            if let Some(idx) = app.nav.r_state.selected() {
                if idx < app.nav.filtered_radio.len() {
                    let (url, title, track) = {
                        let st = &app.data.radio[app.nav.filtered_radio[idx]];
                        (
                            st.stream.clone(),
                            st.title.clone(),
                            st.track.clone().unwrap_or_default(),
                        )
                    };
                    app.run_radio(&url, &title, &track);
                }
            }
        }
        KeyCode::Char('f' | 'F') => {
            if let Some(idx) = app.nav.r_state.selected() {
                if idx < app.nav.filtered_radio.len() {
                    let st = &app.data.radio[app.nav.filtered_radio[idx]];
                    let url = st.stream.clone();
                    let name = st.title.clone();
                    if app.config.favorites.contains(&url) {
                        app.config.favorite_remove(&url);
                    } else {
                        app.config.favorite_add(&url, &name);
                    }
                }
            }
        }
        KeyCode::Esc => app.screen = Screen::RadioCatList,
        _ => {}
    }
}

fn handle_favorites_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => nav_up(&mut app.nav.fav_state, app.config.favorites.len()),
        KeyCode::Down => nav_down(&mut app.nav.fav_state, app.config.favorites.len()),
        KeyCode::Enter => {
            if let Some(idx) = app.nav.fav_state.selected() {
                let favs = app.sorted_favorites();
                if idx < favs.len() {
                    let url = favs[idx].clone();
                    let name =
                        ui::get_name_by_url(&url, &app.data, &app.config).to_string();
                    if is_radio_url(&url, &app.data) {
                        app.run_radio(&url, &name, "");
                    } else {
                        app.run_video(&url, &name, "");
                    }
                }
            }
        }
        KeyCode::Esc => app.screen = Screen::MainMenu,
        _ => {}
    }
}

fn handle_history_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => nav_up(&mut app.nav.hist_state, app.config.history.len()),
        KeyCode::Down => nav_down(&mut app.nav.hist_state, app.config.history.len()),
        KeyCode::Enter => {
            if let Some(idx) = app.nav.hist_state.selected() {
                let history: Vec<_> = app.config.history.iter().rev().collect();
                if idx < history.len() {
                    let url = history[idx].clone();
                    let name =
                        ui::get_name_by_url(&url, &app.data, &app.config).to_string();
                    if is_radio_url(&url, &app.data) {
                        app.run_radio(&url, &name, "");
                    } else {
                        app.run_video(&url, &name, "");
                    }
                }
            }
        }
        KeyCode::Esc => app.screen = Screen::MainMenu,
        _ => {}
    }
}

fn handle_settings_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => nav_up(&mut app.nav.set_state, SETTINGS_COUNT),
        KeyCode::Down => nav_down(&mut app.nav.set_state, SETTINGS_COUNT),
        KeyCode::Enter => {
            let idx = app.nav.set_state.selected().unwrap_or(0);
            match idx {
                0 | 1 | 3 | 6 => {
                    app.nav.edit_buf = app.settings_value(idx);
                    app.screen = Screen::SettingsEdit(idx);
                }
                2 | 4 | 5 | 7 | 8 => {
                    app.settings_toggle(idx);
                }
                _ => {}
            }
        }
        KeyCode::Esc => app.screen = Screen::MainMenu,
        _ => {}
    }
}

fn handle_settings_edit_input(app: &mut App, code: KeyCode, field: usize) {
    match code {
        KeyCode::Char(c) => app.nav.edit_buf.push(c),
        KeyCode::Backspace => {
            app.nav.edit_buf.pop();
        }
        KeyCode::Enter => {
            let val = app.nav.edit_buf.clone();
            app.settings_apply(field, &val);
            app.status_msg = Some("Saved".into());
            app.screen = Screen::Settings;
        }
        KeyCode::Esc => {
            app.nav.edit_buf.clear();
            app.screen = Screen::Settings;
        }
        _ => {}
    }
}

fn handle_detail_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => nav_up(&mut app.nav.epg_state, app.detail.programs.len()),
        KeyCode::Down => nav_down(&mut app.nav.epg_state, app.detail.programs.len()),
        KeyCode::Enter => app.detail_play_selected(),
        KeyCode::Char('l') => app.detail_play_live(),
        KeyCode::Char('f') => {
            if let Some(ch_idx) = app.detail.channel.filter(|&i| i < app.data.channels.len()) {
                let ch = &app.data.channels[ch_idx];
                let url = ch.url.clone();
                let name = ch.name.clone();
                if app.config.favorites.contains(&url) {
                    app.config.favorite_remove(&url);
                } else {
                    app.config.favorite_add(&url, &name);
                }
            }
        }
        KeyCode::Esc => {
            app.screen = app.detail.return_screen.take().unwrap_or(Screen::ChanList);
        }
        _ => {}
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

fn switch_playing_channel(app: &mut App, up: bool) {
    match app.screen {
        Screen::ChanList | Screen::Detail => {
            if up {
                nav_up(&mut app.nav.ch_state, app.nav.filtered.len());
            } else {
                nav_down(&mut app.nav.ch_state, app.nav.filtered.len());
            }
            if let Some(idx) = app.nav.ch_state.selected() {
                if idx < app.nav.filtered.len() {
                    let ch = &app.data.channels[app.nav.filtered[idx]];
                    let url = ch.url.clone();
                    let name = ch.name.clone();
                    app.run_video(&url, &name, "");
                }
            }
        }
        Screen::RadioList => {
            if up {
                nav_up(&mut app.nav.r_state, app.nav.filtered_radio.len());
            } else {
                nav_down(&mut app.nav.r_state, app.nav.filtered_radio.len());
            }
            if let Some(idx) = app.nav.r_state.selected() {
                if idx < app.nav.filtered_radio.len() {
                    let st = &app.data.radio[app.nav.filtered_radio[idx]];
                    let url = st.stream.clone();
                    let name = st.title.clone();
                    let track = st.track.clone().unwrap_or_default();
                    app.run_radio(&url, &name, &track);
                }
            }
        }
        Screen::Favorites => {
            if up {
                nav_up(&mut app.nav.fav_state, app.config.favorites.len());
            } else {
                nav_down(&mut app.nav.fav_state, app.config.favorites.len());
            }
            if let Some(idx) = app.nav.fav_state.selected() {
                let favs = app.sorted_favorites();
                if idx < favs.len() {
                    let url = favs[idx].clone();
                    let name = crate::ui::get_name_by_url(&url, &app.data, &app.config).to_string();
                    app.run_video(&url, &name, "");
                }
            }
        }
        Screen::History => {
            if up {
                nav_up(&mut app.nav.hist_state, app.config.history.len());
            } else {
                nav_down(&mut app.nav.hist_state, app.config.history.len());
            }
            if let Some(idx) = app.nav.hist_state.selected() {
                if idx < app.config.history.len() {
                    let url = app.config.history[idx].clone();
                    let name = crate::ui::get_name_by_url(&url, &app.data, &app.config).to_string();
                    app.run_video(&url, &name, "");
                }
            }
        }
        _ => {}
    }
}

fn is_radio_url(url: &str, data: &crate::models::AppData) -> bool {
    data.radio.iter().any(|r| r.stream == url || r.quality_urls.values().any(|u| u == url))
}

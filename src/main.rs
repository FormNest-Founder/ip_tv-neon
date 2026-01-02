use std::path::Path;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod app;
mod epg;
mod models;
mod ui;
mod utils;

use app::App;
use epg::{scan_local_playlists, update_data};
use models::{Config, Screen};
use ui::ui;
use utils::CACHE_DIR;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long)]
    debug: bool,
}
#[derive(Subcommand)]
enum Commands {
    Update,
    Run,
}

fn set_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        hook(info);
    }));
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    set_panic_hook();
    let config = Config::load();

    if let Some(Commands::Update) = cli.command {
        if update_data(&config).await.is_err() {
            std::process::exit(1);
        } else {
            return Ok(());
        }
    }

    if !Path::new(CACHE_DIR).join("data.bin").exists() {
        let _ = update_data(&config).await;
    }

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        event::EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let mut app = App::new(config);

    loop {
        terminal.draw(|f| ui(f, &mut app))?;
        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Paste(text) => {
                    if app.screen == Screen::Input || app.screen == Screen::LinkInput {
                        app.in_buf.push_str(&text);
                    }
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match app.screen {
                        Screen::Updating => {}
                        Screen::MainMenu => match key.code {
                            KeyCode::Up => {
                                let i = app.m_state.selected().unwrap_or(0);
                                app.m_state.select(Some(if i == 0 { 9 } else { i - 1 }));
                            }
                            KeyCode::Down => {
                                let i = app.m_state.selected().unwrap_or(0);
                                app.m_state.select(Some(if i == 9 { 0 } else { i + 1 }));
                            }
                            KeyCode::Char('r') => {
                                app.r_cat_state.select(Some(0));
                                app.screen = Screen::RadioCatList;
                            }
                            KeyCode::Enter => match app.m_state.selected().unwrap_or(0) {
                                0 => app.screen = Screen::CatList,
                                1 => {
                                    app.r_cat_state.select(Some(0));
                                    app.screen = Screen::RadioCatList;
                                }
                                2 => {
                                    app.local_files = scan_local_playlists();
                                    app.l_state.select(Some(0));
                                    app.screen = Screen::LocalList;
                                }
                                3 => {
                                    app.in_buf.clear();
                                    app.in_tgt = "URL/Magnet".into();
                                    app.screen = Screen::LinkInput;
                                }
                                4 => {
                                    app.filtered = app
                                        .data
                                        .channels
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, c)| app.config.favorites.contains(&c.url))
                                        .map(|(i, _)| i)
                                        .collect();
                                    app.ch_state.select(Some(0));
                                    app.screen = Screen::ChanList;
                                }
                                5 => {
                                    app.filtered = app
                                        .data
                                        .channels
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, c)| app.config.history.contains(&c.url))
                                        .map(|(i, _)| i)
                                        .collect();
                                    app.ch_state.select(Some(0));
                                    app.screen = Screen::ChanList;
                                }
                                6 => app.stop_all(),
                                7 => {
                                    app.screen = Screen::Updating;
                                    terminal.draw(|f| ui(f, &mut app))?;
                                    if update_data(&app.config).await.is_ok() {
                                        app = App::new(Config::load());
                                    }
                                    app.screen = Screen::MainMenu;
                                }
                                8 => app.screen = Screen::Settings,
                                9 => app.quit = true,
                                _ => {}
                            },
                            KeyCode::Esc => app.quit = true,
                            _ => {}
                        },
                        Screen::CatList => match key.code {
                            KeyCode::Up => {
                                let i = app.cat_state.selected().unwrap_or(0);
                                let l = app.data.groups.len();
                                if l > 0 {
                                    app.cat_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.cat_state.selected().unwrap_or(0);
                                let l = app.data.groups.len();
                                if l > 0 {
                                    app.cat_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(idx) = app.cat_state.selected() {
                                    let g = &app.data.groups[idx];
                                    app.filtered = app
                                        .data
                                        .channels
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, c)| &c.group == g)
                                        .map(|(i, _)| i)
                                        .collect();
                                    app.ch_state.select(Some(0));
                                    app.screen = Screen::ChanList;
                                }
                            }
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            _ => {}
                        },
                        Screen::ChanList => match key.code {
                            KeyCode::Up => {
                                let i = app.ch_state.selected().unwrap_or(0);
                                let l = app.filtered.len();
                                if l > 0 {
                                    app.ch_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.ch_state.selected().unwrap_or(0);
                                let l = app.filtered.len();
                                if l > 0 {
                                    app.ch_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(idx) = app.ch_state.selected() {
                                    if let Some(&real_idx) = app.filtered.get(idx) {
                                        if let Some(ch) = app.data.channels.get(real_idx) {
                                            let url = ch.url.clone();
                                            let name = ch.name.clone();
                                            app.run_mpv(&url, &name, "", false);
                                            app.quit = true;
                                        }
                                    }
                                }
                            }
                            KeyCode::Esc | KeyCode::Left => {
                                // If we came from Categories (index 0), go back to CatList.
                                // Otherwise (Favorites, History), go to MainMenu.
                                if app.m_state.selected().unwrap_or(0) == 0 {
                                    app.screen = Screen::CatList;
                                } else {
                                    app.screen = Screen::MainMenu;
                                }
                            }
                            KeyCode::Right => {
                                if !app.filtered.is_empty() {
                                    app.d_state.select(Some(0)); // Reset detail selection
                                    app.screen = Screen::Detail;
                                }
                            }
                            KeyCode::Char(c) => {
                                app.search.push(c);
                                let q = app.search.to_lowercase();
                                let sel_cat = app.cat_state.selected().unwrap_or(0);
                                let g = if app.m_state.selected().unwrap_or(0) == 0 && sel_cat < app.data.groups.len() {
                                    Some(&app.data.groups[sel_cat])
                                } else {
                                    None
                                };
                                app.filtered = app
                                    .data
                                    .channels
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, ch)| {
                                        let in_grp = if let Some(grp) = g { &ch.group == grp } else { true }; // Simple logic, refine if needed for Favorites
                                        in_grp && ch.name.to_lowercase().contains(&q)
                                    })
                                    .map(|(i, _)| i)
                                    .collect();
                                app.ch_state.select(Some(0));
                            }
                            KeyCode::Backspace => {
                                app.search.pop();
                                let q = app.search.to_lowercase();
                                let sel_cat = app.cat_state.selected().unwrap_or(0);
                                let g = if app.m_state.selected().unwrap_or(0) == 0 && sel_cat < app.data.groups.len() {
                                    Some(&app.data.groups[sel_cat])
                                } else {
                                    None
                                };
                                app.filtered = app
                                    .data
                                    .channels
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, ch)| {
                                        let in_grp = if let Some(grp) = g { &ch.group == grp } else { true };
                                        in_grp && ch.name.to_lowercase().contains(&q)
                                    })
                                    .map(|(i, _)| i)
                                    .collect();
                                app.ch_state.select(Some(0));
                            }
                            _ => {}
                        },
                        Screen::Detail => match key.code {
                            KeyCode::Up => {
                                if let Some(idx) = app.ch_state.selected() {
                                    if let Some(&real_idx) = app.filtered.get(idx) {
                                        if let Some(ch) = app.data.channels.get(real_idx) {
                                            // Resolve EPG ID (logic duplicated from epg.rs/ui.rs for simplicity or need public helper)
                                            // Actually we can't easily call find_epg_id here without importing or duplicating.
                                            // Let's assume we can access it. We need to import find_epg_id in main.rs if not already.
                                            // It is not imported. We need to fix imports or duplicate logic.
                                            // Let's duplicate simple lookup: check tvg_id then norm_name map.
                                            let id_opt = if let Some(id) = &ch.tvg_id {
                                                 if app.data.epg.contains_key(id) { Some(id.clone()) } else { None }
                                            } else { None }
                                            .or_else(|| app.data.name_to_id.get(&ch.norm_name).cloned());

                                            if let Some(id) = id_opt {
                                                if let Some(progs) = app.data.epg.get(&id) {
                                                    let l = progs.len();
                                                    let cur = app.d_state.selected().unwrap_or(0);
                                                    if l > 0 {
                                                        app.d_state.select(Some(if cur == 0 { l - 1 } else { cur - 1 }));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Down => {
                                if let Some(idx) = app.ch_state.selected() {
                                    if let Some(&real_idx) = app.filtered.get(idx) {
                                        if let Some(ch) = app.data.channels.get(real_idx) {
                                             let id_opt = if let Some(id) = &ch.tvg_id {
                                                 if app.data.epg.contains_key(id) { Some(id.clone()) } else { None }
                                            } else { None }
                                            .or_else(|| app.data.name_to_id.get(&ch.norm_name).cloned());

                                            if let Some(id) = id_opt {
                                                if let Some(progs) = app.data.epg.get(&id) {
                                                    let l = progs.len();
                                                    let cur = app.d_state.selected().unwrap_or(0);
                                                    if l > 0 {
                                                        app.d_state.select(Some(if cur == l - 1 { 0 } else { cur + 1 }));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(idx) = app.ch_state.selected() {
                                    if let Some(&real_idx) = app.filtered.get(idx) {
                                        if let Some(ch) = app.data.channels.get(real_idx) {
                                            let mut url = ch.url.clone();
                                            let mut prog_title = String::new();
                                            
                                            // Try to find the specific program selected in Detail view
                                            let id_opt = if let Some(id) = &ch.tvg_id {
                                                 if app.data.epg.contains_key(id) { Some(id.clone()) } else { None }
                                            } else { None }
                                            .or_else(|| app.data.name_to_id.get(&ch.norm_name).cloned());

                                            if let Some(id) = id_opt {
                                                if let Some(progs) = app.data.epg.get(&id) {
                                                    let sel_prog_idx = app.d_state.selected().unwrap_or(0);
                                                    if let Some(p) = progs.get(sel_prog_idx) {
                                                        prog_title = p.title.clone();
                                                        let now = chrono::Utc::now().timestamp();
                                                        // If program started in the past, treat as archive/timeshift
                                                        if p.start < now {
                                                            // Common archive format: ?utc=START&lutc=NOW
                                                            // Some providers use just ?utc=START
                                                            // We will append ?utc={start}&lutc={now}
                                                            if url.contains('?') {
                                                                url = format!("{}&utc={}&lutc={}", url, p.start, now);
                                                            } else {
                                                                url = format!("{}?utc={}&lutc={}", url, p.start, now);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            
                                            let ch_name = ch.name.clone();
                                            app.run_mpv(&url, &ch_name, &prog_title, false);
                                            app.quit = true;
                                        }
                                    }
                                }
                            }
                            KeyCode::Esc | KeyCode::Left => app.screen = Screen::ChanList,
                            _ => {}
                        },
                        Screen::RadioCatList => match key.code {
                            KeyCode::Up => {
                                let i = app.r_cat_state.selected().unwrap_or(0);
                                let l = app.data.radio_groups.len();
                                if l > 0 {
                                    app.r_cat_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.r_cat_state.selected().unwrap_or(0);
                                let l = app.data.radio_groups.len();
                                if l > 0 {
                                    app.r_cat_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                app.r_state.select(Some(0));
                                app.screen = Screen::RadioList;
                            }
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            _ => {}
                        },
                        Screen::RadioList => match key.code {
                            KeyCode::Up => {
                                let i = app.r_state.selected().unwrap_or(0);
                                let cat_idx = app.r_cat_state.selected().unwrap_or(0);
                                let category = &app.data.radio_groups[cat_idx];
                                let l = app
                                    .data
                                    .radio
                                    .iter()
                                    .filter(|r| {
                                        category == "All"
                                            || r.genres
                                                .iter()
                                                .any(|g| g.to_uppercase() == category.to_uppercase())
                                    })
                                    .count();
                                if l > 0 {
                                    app.r_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.r_state.selected().unwrap_or(0);
                                let cat_idx = app.r_cat_state.selected().unwrap_or(0);
                                let category = &app.data.radio_groups[cat_idx];
                                let l = app
                                    .data
                                    .radio
                                    .iter()
                                    .filter(|r| {
                                        category == "All"
                                            || r.genres
                                                .iter()
                                                .any(|g| g.to_uppercase() == category.to_uppercase())
                                    })
                                    .count();
                                if l > 0 {
                                    app.r_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(i) = app.r_state.selected() {
                                    let cat_idx = app.r_cat_state.selected().unwrap_or(0);
                                    let category = &app.data.radio_groups[cat_idx];
                                    let filtered: Vec<_> = app
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
                                    if let Some(s) = filtered.get(i) {
                                        let stream = s.stream.clone();
                                        let track = s.track.clone().unwrap_or_default();
                                        // User format: "Station | Artist - Song"
                                        let title = if !track.is_empty() {
                                            format!("{} | {}", s.title, track)
                                        } else {
                                            s.title.clone()
                                        };
                                        app.run_mpv(&stream, &title, &track, true);
                                        app.quit = true;
                                    }
                                }
                            }
                            KeyCode::Esc => app.screen = Screen::RadioCatList,
                            _ => {}
                        },
                        _ => {}
                    }
                    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.quit = true;
                    }
                }
                _ => {}
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
    terminal.show_cursor()?;
    Ok(())
}
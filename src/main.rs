use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
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
use utils::get_cache_dir;

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
    let cache_dir = get_cache_dir();

    if let Some(Commands::Update) = cli.command {
        if update_data(&config).await.is_err() {
            std::process::exit(1);
        } else {
            return Ok(());
        }
    }

    if !cache_dir.join("data.bin").exists() {
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

    let menu_items_count = 10;

    loop {
        terminal.draw(|f| ui(f, &mut app))?;
        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Paste(text) => {
                    if app.screen == Screen::Input || app.screen == Screen::LinkInput {
                        app.in_buf.push_str(&text);
                    }
                }
                Event::Key(key) => {
                    match app.screen {
                        Screen::Updating => {}
                        Screen::MainMenu => match key.code {
                            KeyCode::Up => {
                                let i = app.m_state.selected().unwrap_or(0);
                                app.m_state.select(Some(if i == 0 {
                                    menu_items_count - 1
                                } else {
                                    i - 1
                                }));
                            }
                            KeyCode::Down => {
                                let i = app.m_state.selected().unwrap_or(0);
                                app.m_state.select(Some(if i == menu_items_count - 1 {
                                    0
                                } else {
                                    i + 1
                                }));
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
                                    app.cat_state
                                        .select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.cat_state.selected().unwrap_or(0);
                                let l = app.data.groups.len();
                                if l > 0 {
                                    app.cat_state
                                        .select(Some(if i == l - 1 { 0 } else { i + 1 }));
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
                                    app.ch_state
                                        .select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.ch_state.selected().unwrap_or(0);
                                let l = app.filtered.len();
                                if l > 0 {
                                    app.ch_state
                                        .select(Some(if i == l - 1 { 0 } else { i + 1 }));
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
                                if app.m_state.selected().unwrap_or(0) == 0 {
                                    app.screen = Screen::CatList;
                                } else {
                                    app.screen = Screen::MainMenu;
                                }
                            }
                            KeyCode::Right => {
                                if !app.filtered.is_empty() {
                                    app.d_state.select(Some(0));
                                    app.screen = Screen::Detail;
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
                            _ => {}
                        },
                        Screen::Detail => match key.code {
                            KeyCode::Up => {
                                if let Some(progs) = app.get_current_progs() {
                                    let l = progs.len();
                                    let cur = app.d_state.selected().unwrap_or(0);
                                    if l > 0 {
                                        app.d_state.select(Some(if cur == 0 {
                                            l - 1
                                        } else {
                                            cur - 1
                                        }));
                                    }
                                }
                            }
                            KeyCode::Down => {
                                if let Some(progs) = app.get_current_progs() {
                                    let l = progs.len();
                                    let cur = app.d_state.selected().unwrap_or(0);
                                    if l > 0 {
                                        app.d_state.select(Some(if cur == l - 1 {
                                            0
                                        } else {
                                            cur + 1
                                        }));
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(idx) = app.ch_state.selected() {
                                    if let Some(&real_idx) = app.filtered.get(idx) {
                                        if let Some(ch) = app.data.channels.get(real_idx) {
                                            let mut url = ch.url.clone();
                                            let mut prog_title = String::new();

                                            if let Some(progs) = app.get_current_progs() {
                                                let sel_prog_idx =
                                                    app.d_state.selected().unwrap_or(0);
                                                if let Some(p) = progs.get(sel_prog_idx) {
                                                    prog_title = p.title.clone();
                                                    let now = chrono::Utc::now().timestamp();
                                                    if p.start < now {
                                                        let sep = if url.contains('?') {
                                                            "&"
                                                        } else {
                                                            "?"
                                                        };
                                                        url = format!(
                                                            "{}{}utc={}&lutc={}",
                                                            url, sep, p.start, now
                                                        );
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
                                    app.r_cat_state.select(Some(if i == 0 {
                                        l - 1
                                    } else {
                                        i - 1
                                    }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.r_cat_state.selected().unwrap_or(0);
                                let l = app.data.radio_groups.len();
                                if l > 0 {
                                    app.r_cat_state.select(Some(if i == l - 1 {
                                        0
                                    } else {
                                        i + 1
                                    }));
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
                                let l = app.get_radio_filtered_count();
                                if l > 0 {
                                    app.r_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.r_state.selected().unwrap_or(0);
                                let l = app.get_radio_filtered_count();
                                if l > 0 {
                                    app.r_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(s) = app.get_selected_radio() {
                                    let stream = s.stream.clone();
                                    let track = s.track.clone().unwrap_or_default();
                                    let title = if !track.is_empty() {
                                        format!("{} | {}", s.title, track)
                                    } else {
                                        s.title.clone()
                                    };
                                    app.run_mpv(&stream, &title, &track, true);
                                    app.quit = true;
                                }
                            }
                            KeyCode::Esc => app.screen = Screen::RadioCatList,
                            _ => {}
                        },
                        Screen::LocalList => match key.code {
                            KeyCode::Up => {
                                let i = app.l_state.selected().unwrap_or(0);
                                let l = app.local_files.len();
                                if l > 0 {
                                    app.l_state.select(Some(if i == 0 { l - 1 } else { i - 1 }));
                                }
                            }
                            KeyCode::Down => {
                                let i = app.l_state.selected().unwrap_or(0);
                                let l = app.local_files.len();
                                if l > 0 {
                                    app.l_state.select(Some(if i == l - 1 { 0 } else { i + 1 }));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(i) = app.l_state.selected() {
                                    if let Some(path) = app.local_files.get(i) {
                                        app.config.playlist_url = path.to_string_lossy().into();
                                        let _ = app.config.save();
                                        app.screen = Screen::Updating;
                                        terminal.draw(|f| ui(f, &mut app))?;
                                        if update_data(&app.config).await.is_ok() {
                                            app = App::new(Config::load());
                                        }
                                        app.screen = Screen::MainMenu;
                                    }
                                }
                            }
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            _ => {}
                        },
                        Screen::LinkInput => match key.code {
                            KeyCode::Enter => {
                                let url = app.in_buf.clone();
                                if !url.is_empty() {
                                    app.run_mpv(&url, "LINK", "", false);
                                    app.quit = true;
                                }
                            }
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            KeyCode::Char(c) => {
                                app.in_buf.push(c);
                            }
                            KeyCode::Backspace => {
                                app.in_buf.pop();
                            }
                            _ => {}
                        },
                        Screen::Settings => match key.code {
                            KeyCode::Up => {
                                let i = app.s_state.selected().unwrap_or(0);
                                app.s_state.select(Some(if i == 0 { 2 } else { i - 1 }));
                            }
                            KeyCode::Down => {
                                let i = app.s_state.selected().unwrap_or(0);
                                app.s_state.select(Some(if i == 2 { 0 } else { i + 1 }));
                            }
                            KeyCode::Enter => match app.s_state.selected().unwrap_or(0) {
                                0 => {
                                    app.in_buf = app.config.playlist_url.clone();
                                    app.in_tgt = "Playlist URL".into();
                                    app.screen = Screen::Input;
                                }
                                1 => {
                                    app.in_buf = app.config.epg_url.clone();
                                    app.in_tgt = "EPG URL".into();
                                    app.screen = Screen::Input;
                                }
                                2 => {
                                    let _ = app.config.save();
                                    app.screen = Screen::MainMenu;
                                }
                                _ => {}
                            },
                            KeyCode::Esc => app.screen = Screen::MainMenu,
                            _ => {}
                        },
                        Screen::Input => match key.code {
                            KeyCode::Enter => {
                                let val = app.in_buf.clone();
                                if app.in_tgt == "Playlist URL" {
                                    app.config.playlist_url = val;
                                } else if app.in_tgt == "EPG URL" {
                                    app.config.epg_url = val;
                                }
                                app.screen = Screen::Settings;
                            }
                            KeyCode::Esc => app.screen = Screen::Settings,
                            KeyCode::Char(c) => {
                                app.in_buf.push(c);
                            }
                            KeyCode::Backspace => {
                                app.in_buf.pop();
                            }
                            _ => {}
                        },
                    }
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
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

// Helper methods for App to keep main clean
impl App {
    fn update_filter(&mut self) {
        let q = self.search.to_lowercase();
        let sel_cat = self.cat_state.selected().unwrap_or(0);
        let g = if self.m_state.selected().unwrap_or(0) == 0 && sel_cat < self.data.groups.len() {
            Some(&self.data.groups[sel_cat])
        } else {
            None
        };
        self.filtered = self
            .data
            .channels
            .iter()
            .enumerate()
            .filter(|(_, ch)| {
                let in_grp = if let Some(grp) = g {
                    &ch.group == grp
                } else {
                    true
                };
                in_grp && ch.name.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        self.ch_state.select(Some(0));
    }

    fn get_current_progs(&self) -> Option<&Vec<models::EpgProgram>> {
        let idx = self.ch_state.selected()?;
        let &real_idx = self.filtered.get(idx)?;
        let ch = self.data.channels.get(real_idx)?;
        let id = crate::epg::find_epg_id(ch, &self.data)?;
        self.data.epg.get(&id)
    }

    fn get_radio_filtered_count(&self) -> usize {
        let cat_idx = self.r_cat_state.selected().unwrap_or(0);
        if self.data.radio_groups.is_empty() {
            return 0;
        }
        let category = &self.data.radio_groups[cat_idx];
        self.data
            .radio
            .iter()
            .filter(|r| {
                category == "All"
                    || r.genres
                        .iter()
                        .any(|g| g.to_uppercase() == category.to_uppercase())
            })
            .count()
    }

    fn get_selected_radio(&self) -> Option<&models::RadioStation> {
        let i = self.r_state.selected()?;
        let cat_idx = self.r_cat_state.selected().unwrap_or(0);
        if self.data.radio_groups.is_empty() {
            return None;
        }
        let category = &self.data.radio_groups[cat_idx];
        self.data
            .radio
            .iter()
            .filter(|r| {
                category == "All"
                    || r.genres
                        .iter()
                        .any(|g| g.to_uppercase() == category.to_uppercase())
            })
            .nth(i)
    }
}

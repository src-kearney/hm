use crossterm::{
    cursor::{self, SetCursorStyle},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::io::{self, stdout, Write};
use std::mem;

use crate::{capture_hashes, do_capture, do_delete, do_push, git_silent, hm_path, load_config, Config, SEP};

enum Mode {
    Browse,
    Capture(String),
    Message(String),
}

struct App {
    config: Config,
    entries: Vec<(String, String, String)>, // (hash, ts, text)
    list_state: ListState,
    mode: Mode,
}

impl App {
    fn new(config: Config) -> Self {
        let entries = load_entries(&config);
        let mut list_state = ListState::default();
        if !entries.is_empty() {
            list_state.select(Some(0));
        }
        App {
            config,
            entries,
            list_state,
            mode: Mode::Browse,
        }
    }

    fn reload(&mut self) {
        let prev = self.list_state.selected();
        self.entries = load_entries(&self.config);
        let next = prev
            .map(|s| s.min(self.entries.len().saturating_sub(1)))
            .filter(|_| !self.entries.is_empty());
        self.list_state.select(next);
    }

    fn move_up(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0).saturating_sub(1);
        self.list_state.select(Some(i));
    }

    fn move_down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some((i + 1).min(self.entries.len() - 1)));
    }

    fn move_top(&mut self) {
        if !self.entries.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn move_bottom(&mut self) {
        if !self.entries.is_empty() {
            self.list_state.select(Some(self.entries.len() - 1));
        }
    }
}

fn load_entries(config: &Config) -> Vec<(String, String, String)> {
    let path = hm_path(config);
    let hashes = capture_hashes(config);
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            let hash = hashes.get(i).cloned().unwrap_or_else(|| "???????".to_string());
            match l.find(SEP) {
                Some(pos) => (hash, l[..pos].to_string(), l[pos + SEP.len()..].to_string()),
                None => (hash, String::new(), l.to_string()),
            }
        })
        .collect()
}

pub fn run() -> Result<(), String> {
    let config = load_config()?;
    let mut app = App::new(config);

    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut out = stdout();
    // Hide the terminal cursor since we use a software cursor in capture mode.
    // E-ink displays ghost badly with cursor blink so this avoids forced partial refreshes.
    execute!(out, EnterAlternateScreen, cursor::Hide).map_err(|e| e.to_string())?;

    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let result = event_loop(&mut terminal, &mut app);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show).ok();

    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), String> {
    loop {
        // This swaps list_state out temporarily.
        // render_stateful_widget needs &mut State but draw() also borrows app for entries/mode.
        let mut ls = mem::take(&mut app.list_state);
        terminal
            .draw(|f| draw(f, app, &mut ls))
            .map_err(|e| e.to_string())?;
        app.list_state = ls;

        // Block until input.  Zero spurious redraws for e-ink targets.
        let ev = event::read().map_err(|e| e.to_string())?;

        if process_event(terminal, app, ev)? {
            return Ok(());
        }
    }
}

fn draw(f: &mut Frame, app: &App, list_state: &mut ListState) {
    let area = f.area();

    let footer_height: u16 = match &app.mode {
        Mode::Browse => 1,
        Mode::Capture(_) | Mode::Message(_) => 2,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(footer_height)])
        .split(area);

    // List
    let n = app.entries.len();
    let title = match n {
        0 => " hm ".to_string(),
        1 => " hm  1 entry ".to_string(),
        _ => format!(" hm  {} entries ", n),
    };

    let sel = list_state.selected();
    let items: Vec<ListItem> = app
        .entries
        .iter()
        .enumerate()
        .map(|(i, (hash, ts, text))| {
            let marker = if sel == Some(i) { ">" } else { " " };
            let line = format!("{} {}  {}  {}", marker, hash, ts, text);
            let style = if sel == Some(i) {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_stateful_widget(list, chunks[0], list_state);

    // Footer
    let footer: Paragraph = match &app.mode {
        Mode::Browse => {
            let hint = if n == 0 {
                " n  new    q  quit"
            } else {
                " n  new    e  edit    d  delete    p  push    q  quit"
            };
            Paragraph::new(hint)
        }
        Mode::Capture(input) => Paragraph::new(vec![
            Line::from(format!(" > {}|", input)),
            Line::from(" Enter  save    Esc  cancel"),
        ]),
        Mode::Message(msg) => Paragraph::new(vec![
            Line::from(format!(" {}", msg)),
            Line::from(" any key"),
        ]),
    };

    f.render_widget(footer, chunks[1]);
}

fn process_event(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    ev: Event,
) -> Result<bool, String> {
    let Event::Key(key) = ev else {
        return Ok(false);
    };

    // Only act on key presses, not releases or repeats.
    if key.kind != KeyEventKind::Press {
        return Ok(false);
    }

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }

    // Take the mode out so we can call &mut app methods without aliasing app.mode.
    let mode = mem::replace(&mut app.mode, Mode::Browse);

    match mode {
        Mode::Browse => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('n') => app.mode = Mode::Capture(String::new()),
            KeyCode::Char('e') => {
                if !app.entries.is_empty() {
                    open_vim(terminal, app)?;
                }
            }
            KeyCode::Char('p') => {
                app.mode = match do_push(&app.config) {
                    Ok(()) => Mode::Message("Pushed.".to_string()),
                    Err(e) => Mode::Message(format!("Push failed: {}", e)),
                };
            }
            KeyCode::Char('d') => {
                if let Some(i) = app.list_state.selected() {
                    let hash = app.entries.get(i).map(|(h, _, _)| h.clone()).unwrap_or_default();
                    if !hash.is_empty() {
                        app.mode = match do_delete(&hash, &app.config) {
                            Ok(()) => {
                                app.reload();
                                Mode::Message(format!("Deleted {}.", hash))
                            }
                            Err(e) => Mode::Message(e),
                        };
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
            KeyCode::Char('g') | KeyCode::Home => app.move_top(),
            KeyCode::Char('G') | KeyCode::End => app.move_bottom(),
            _ => {}
        },

        Mode::Capture(mut input) => match key.code {
            KeyCode::Esc => { /* drop input, mode stays Browse */ }
            KeyCode::Enter => {
                let text = input.trim().to_string();
                if !text.is_empty() {
                    app.mode = match do_capture(&text, &app.config) {
                        Ok(ts) => {
                            app.reload();
                            app.list_state.select(Some(0));
                            Mode::Message(format!("Saved  {}", ts))
                        }
                        Err(e) => Mode::Message(format!("Error: {}", e)),
                    };
                }
            }
            KeyCode::Backspace => {
                input.pop();
                app.mode = Mode::Capture(input);
            }
            KeyCode::Char(c) => {
                input.push(c);
                app.mode = Mode::Capture(input);
            }
            _ => app.mode = Mode::Capture(input),
        },

        Mode::Message(_) => { /* any key clears message, mode stays Browse */ }
    }

    Ok(false)
}

fn open_vim(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), String> {
    let idx = match app.list_state.selected() {
        Some(i) => i,
        None => return Ok(()),
    };

    let (_, ts, text) = app.entries[idx].clone();

    // Write just this entry's text to a temp file so vim edits one thought.
    let tmp = std::env::temp_dir().join("hm_edit.txt");
    std::fs::write(&tmp, format!("{}\n", text))
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| e.to_string())?;
    // Write cursor reset directly to stdout and flush before vim takes over.
    let mut out = stdout();
    execute!(out, cursor::Show, SetCursorStyle::BlinkingBlock).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;

    std::process::Command::new("vim")
        .args(["-c", "colorscheme morning"])
        .arg(&tmp)
        .status()
        .map_err(|e| format!("Failed to launch vim: {}", e))?;

    let new_text = std::fs::read_to_string(&tmp).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp);
    let new_text = new_text.trim().to_string();

    enable_raw_mode().map_err(|e| e.to_string())?;
    execute!(terminal.backend_mut(), EnterAlternateScreen, cursor::Hide)
        .map_err(|e| e.to_string())?;
    terminal.clear().map_err(|e| e.to_string())?;

    if !new_text.is_empty() && new_text != text {
        let path = hm_path(&app.config);
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let new_entry = if ts.is_empty() {
            new_text
        } else {
            format!("{}{}{}", ts, SEP, new_text)
        };
        let new_content = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, l)| if i == idx { new_entry.clone() } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, new_content)
            .map_err(|e| format!("Failed to write file: {}", e))?;
        git_silent(&app.config.repo, &["add", &app.config.file])?;
        let msg = format!("edit: {}", crate::preview(&text));
        git_silent(&app.config.repo, &["commit", "-m", &msg])?;
    }

    app.reload();

    Ok(())
}

//! TUI entry point + screen dispatcher.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Args;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use tokio::sync::mpsc;

use crate::auth::conn::{self, ConnState};
use crate::bindings::*;
use crate::config::Config;

#[derive(Debug, Args, Default)]
pub struct TuiArgs {
    /// Skip the first-run login flow (assume credentials already exist).
    #[arg(long)]
    pub skip_login: bool,
}

pub async fn run(config: Arc<Config>, _args: TuiArgs) -> Result<ExitCode> {
    // If we don't have credentials, run the interactive login flow first.
    let creds = crate::store::credentials::Credentials::load(&config.credentials_file())?;
    let state = match creds {
        Some(creds) => match conn::connect(&config, Some(creds.token)) {
            Ok(s) => s,
            Err(err) => return run_connection_error(&config, &err).await,
        },
        None => {
            // The "first run" path is intentionally CLI-driven for now.
            // The TUI itself shows a "please run `spacenix login`" screen.
            return run_first_run(&config).await;
        }
    };

    let terminal = ratatui::init();
    let app_result = App::new(config, state).run(terminal).await;
    ratatui::restore();
    app_result.map(|()| ExitCode::from(0))
}

async fn run_connection_error(config: &Config, err: &anyhow::Error) -> Result<ExitCode> {
    eprintln!("Could not connect to SpacetimeDB.");
    eprintln!();
    eprintln!("  Server:  {}", config.stdb_uri);
    eprintln!("  Module:  {}", config.stdb_module);
    eprintln!();
    eprintln!("Error: {err:#}");
    eprintln!();
    eprintln!("Make sure the server is running and the module is published.");
    Ok(ExitCode::from(1))
}

async fn run_first_run(config: &Config) -> Result<ExitCode> {
    // No credentials — the TUI can't function yet. Print a friendly message
    // and exit 0 so the user lands on `spacenix login` cleanly.
    eprintln!("You are not signed in yet.");
    eprintln!();
    eprintln!("Run `spacenix login` first. That command will open your browser");
    eprintln!("to sign in, or accept a personal access token via --token <pat>.");
    eprintln!();
    eprintln!("Config directory: {}", config.config_dir.display());
    Ok(ExitCode::from(0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Secrets,
    Sync,
    Tokens,
    Help,
}

struct App {
    #[allow(dead_code)]
    config: Arc<Config>,
    state: ConnState,
    screen: Screen,
    /// Status bar message.
    status: String,
    /// Secrets list state.
    secrets: ListState,
    secrets_items: Vec<SecretRow>,
    /// Sync list state.
    sync: ListState,
    sync_items: Vec<SyncRow>,
    /// Tokens list state.
    tokens: ListState,
    token_items: Vec<TokenRow>,
    should_quit: bool,
    /// Channel of events coming from the input thread.
    events: mpsc::UnboundedReceiver<TuiEvent>,
    /// Toast / modal one-shot state.
    toast: Option<String>,
}

#[derive(Clone)]
struct SecretRow {
    #[allow(dead_code)]
    id: u64,
    env: String,
    devices: String,
    permissions: String,
}

#[derive(Clone)]
struct SyncRow {
    id: u64,
    name: String,
    path: String,
    selected: bool,
    is_directory: bool,
}

#[derive(Clone)]
struct TokenRow {
    id: u64,
    name: String,
    status: String,
    permissions: String,
}

#[derive(Debug)]
enum TuiEvent {
    Input(Event),
    Tick,
}

impl App {
    fn new(config: Arc<Config>, state: ConnState) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        // Spawn the input reader.
        tokio::spawn(async move {
            let mut stream = EventStream::new();
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    ev = stream.next() => {
                        match ev {
                            Some(Ok(event)) => {
                                if tx.send(TuiEvent::Input(event)).is_err() {
                                    return;
                                }
                            }
                            Some(Err(_)) => return,
                            None => return,
                        }
                    }
                    _ = tick.tick() => {
                        if tx.send(TuiEvent::Tick).is_err() {
                            return;
                        }
                    }
                }
            }
        });

        let mut app = Self {
            config,
            state,
            screen: Screen::Secrets,
            status: "ready".to_string(),
            secrets: ListState::default(),
            secrets_items: Vec::new(),
            sync: ListState::default(),
            sync_items: Vec::new(),
            tokens: ListState::default(),
            token_items: Vec::new(),
            should_quit: false,
            events: rx,
            toast: None,
        };
        app.refresh_secrets();
        app.refresh_sync();
        app.refresh_tokens();
        app
    }

    async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        // Subscribe to all the views we need.
        self.state
            .conn
            .subscription_builder()
            .on_applied(|_| tracing::debug!("all-tables subscription applied"))
            .on_error(|_ctx, err| tracing::error!(?err, "subscription error"))
            .subscribe([
                "SELECT * FROM my_secrets",
                "SELECT * FROM my_files",
                "SELECT * FROM my_api_keys",
            ]);
        // Wait briefly for the first subscription update.
        tokio::time::sleep(Duration::from_millis(400)).await;
        self.refresh_secrets();
        self.refresh_sync();
        self.refresh_tokens();

        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            match self.events.recv().await {
                Some(TuiEvent::Input(Event::Key(key))) => self.on_key(key),
                Some(TuiEvent::Input(_)) => {}
                Some(TuiEvent::Tick) => {
                    self.refresh_secrets();
                    self.refresh_sync();
                    self.refresh_tokens();
                }
                None => return Ok(()),
            }
        }
        Ok(())
    }

    fn on_key(&mut self, key: KeyEvent) {
        // Global keys
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab | KeyCode::BackTab => {
                self.screen = match self.screen {
                    Screen::Secrets => Screen::Sync,
                    Screen::Sync => Screen::Tokens,
                    Screen::Tokens => Screen::Help,
                    Screen::Help => Screen::Secrets,
                };
            }
            KeyCode::Char('1') => self.screen = Screen::Secrets,
            KeyCode::Char('2') => self.screen = Screen::Sync,
            KeyCode::Char('3') => self.screen = Screen::Tokens,
            KeyCode::Char('?') => self.screen = Screen::Help,
            _ => match self.screen {
                Screen::Secrets => self.on_key_secrets(key),
                Screen::Sync => self.on_key_sync(key),
                Screen::Tokens => self.on_key_tokens(key),
                Screen::Help => {}
            },
        }
    }

    fn on_key_secrets(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.list_next_secrets(),
            KeyCode::Up | KeyCode::Char('k') => self.list_prev_secrets(),
            _ => {}
        }
    }

    fn on_key_sync(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.list_next_sync(),
            KeyCode::Up | KeyCode::Char('k') => self.list_prev_sync(),
            KeyCode::Char(' ') => self.toggle_sync_selection(),
            _ => {}
        }
    }

    fn on_key_tokens(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.list_next_tokens(),
            KeyCode::Up | KeyCode::Char('k') => self.list_prev_tokens(),
            _ => {}
        }
    }

    fn list_next_secrets(&mut self) {
        let len = self.secrets_items.len();
        if len == 0 {
            return;
        }
        let i = self.secrets.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.secrets.select(Some(i));
    }

    fn list_prev_secrets(&mut self) {
        let len = self.secrets_items.len();
        if len == 0 {
            return;
        }
        let i = self
            .secrets
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.secrets.select(Some(i));
    }

    fn list_next_sync(&mut self) {
        let len = self.sync_items.len();
        if len == 0 {
            return;
        }
        let i = self.sync.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.sync.select(Some(i));
    }

    fn list_prev_sync(&mut self) {
        let len = self.sync_items.len();
        if len == 0 {
            return;
        }
        let i = self
            .sync
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.sync.select(Some(i));
    }

    fn list_next_tokens(&mut self) {
        let len = self.token_items.len();
        if len == 0 {
            return;
        }
        let i = self.tokens.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.tokens.select(Some(i));
    }

    fn list_prev_tokens(&mut self) {
        let len = self.token_items.len();
        if len == 0 {
            return;
        }
        let i = self
            .tokens
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.tokens.select(Some(i));
    }

    fn toggle_sync_selection(&mut self) {
        let Some(idx) = self.sync.selected() else {
            return;
        };
        let Some(row) = self.sync_items.get(idx) else {
            return;
        };
        let id = row.id;
        let mut sel = crate::store::sync::SyncSelection::load(&self.config.sync_file())
            .unwrap_or_default();
        if let Some(file) = self
            .state
            .conn
            .db()
            .my_files()
            .iter()
            .find(|f| f.id == id)
        {
            let sel_file = crate::store::sync::SelectedFile {
                id: file.id,
                path: file.tree_path.clone(),
                name: file.name.clone(),
                is_directory: file.is_directory,
                local_path: None,
                added_at: chrono::Utc::now(),
            };
            sel.toggle(&sel_file);
            if let Err(err) = sel.save(&self.config.sync_file()) {
                self.toast = Some(format!("save failed: {err}"));
                return;
            }
            self.toast = Some(if sel.contains(id) {
                format!("+ synced {}", file.name)
            } else {
                format!("- unsynced {}", file.name)
            });
        }
        self.refresh_sync();
    }

    fn refresh_secrets(&mut self) {
        let mut rows: Vec<SecretRow> = self
            .state
            .conn
            .db()
            .my_secrets()
            .iter()
            .map(|s| SecretRow {
                id: s.id,
                env: s.env,
                devices: if s.device_ids.is_empty() {
                    "all".into()
                } else {
                    s.device_ids.join(",")
                },
                permissions: if s.permissions.is_empty() {
                    "*".into()
                } else {
                    s.permissions.join(",")
                },
            })
            .collect();
        rows.sort_by(|a, b| a.env.cmp(&b.env));
        self.secrets_items = rows;
        if self.secrets.selected().is_none() && !self.secrets_items.is_empty() {
            self.secrets.select(Some(0));
        }
    }

    fn refresh_sync(&mut self) {
        let sel = crate::store::sync::SyncSelection::load(&self.config.sync_file())
            .unwrap_or_default();
        let mut rows: Vec<SyncRow> = self
            .state
            .conn
            .db()
            .my_files()
            .iter()
            .map(|f| SyncRow {
                id: f.id,
                name: f.name.clone(),
                path: f.tree_path.clone().unwrap_or_else(|| "(root)".into()),
                selected: sel.contains(f.id),
                is_directory: f.is_directory,
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        self.sync_items = rows;
        if self.sync.selected().is_none() && !self.sync_items.is_empty() {
            self.sync.select(Some(0));
        }
    }

    fn refresh_tokens(&mut self) {
        let mut rows: Vec<TokenRow> = self
            .state
            .conn
            .db()
            .my_api_keys()
            .iter()
            .map(|k| TokenRow {
                id: k.id,
                name: k.name.clone(),
                status: if k.revoked_at.is_some() {
                    "revoked".into()
                } else {
                    "active".into()
                },
                permissions: k.permissions.join(","),
            })
            .collect();
        rows.sort_by_key(|a| a.id);
        self.token_items = rows;
        if self.tokens.selected().is_none() && !self.token_items.is_empty() {
            self.tokens.select(Some(0));
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(3),
            ])
            .split(area);

        // Header
        let title = Paragraph::new(Line::from(vec![
            Span::styled("SpaceNix", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(
                match self.screen {
                    Screen::Secrets => "1 · Secrets",
                    Screen::Sync => "2 · Sync",
                    Screen::Tokens => "3 · Tokens",
                    Screen::Help => "? · Help",
                },
                Style::default().fg(Color::Yellow),
            ),
        ]))
        .block(Block::default().borders(Borders::ALL).title(" SpaceNix TUI "));
        frame.render_widget(title, chunks[0]);

        // Body
        match self.screen {
            Screen::Secrets => self.render_secrets(frame, chunks[1]),
            Screen::Sync => self.render_sync(frame, chunks[1]),
            Screen::Tokens => self.render_tokens(frame, chunks[1]),
            Screen::Help => self.render_help(frame, chunks[1]),
        }

        // Status bar
        let identity = self
            .state
            .identity()
            .map(|i| i.to_hex().to_string())
            .unwrap_or_else(|| "—".into());
        let short_id = if identity.len() > 12 {
            format!("{}…{}", &identity[..6], &identity[identity.len() - 6..])
        } else {
            identity
        };
        let status_msg = self.toast.clone().unwrap_or_else(|| self.status.clone());
        let status = Paragraph::new(Line::from(vec![
            Span::raw(format!(" identity {}  {}", short_id, status_msg)),
        ]))
        .block(Block::default().borders(Borders::ALL).title(" status "));
        frame.render_widget(status, chunks[2]);
    }

    fn render_secrets(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .secrets_items
            .iter()
            .map(|r| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:<24}", r.env), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" devices={}", r.devices)),
                    Span::raw(format!(" perms={}", r.permissions)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Secrets (Tab to switch) "),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.secrets);
    }

    fn render_sync(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .sync_items
            .iter()
            .map(|r| {
                let mark = if r.selected { "[x]" } else { "[ ]" };
                let kind = if r.is_directory { "d" } else { "f" };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{mark} "), Style::default().fg(Color::Green)),
                    Span::raw(format!("[{kind}] ")),
                    Span::styled(format!("{:<32}", r.name), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" {}", r.path)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Sync selection (space to toggle) "),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.sync);
    }

    fn render_tokens(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .token_items
            .iter()
            .map(|r| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("#{:<6}", r.id), Style::default().fg(Color::Magenta)),
                    Span::styled(format!("{:<24}", r.name), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" {} ", r.status)),
                    Span::raw(format!(" perms={}", r.permissions)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Personal access tokens "),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.tokens);
    }

    fn render_help(&mut self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from("SpaceNix TUI — quick reference"),
            Line::from(""),
            Line::from("  1 / Tab       switch to the Secrets screen"),
            Line::from("  2 / Shift+Tab switch to the Sync screen"),
            Line::from("  3            switch to the Tokens screen"),
            Line::from("  ?            this help screen"),
            Line::from("  ↑/↓  j/k     move selection"),
            Line::from("  Space        toggle sync on/off (sync screen)"),
            Line::from("  q / Ctrl+C   quit"),
            Line::from(""),
            Line::from("Headless usage:"),
            Line::from("  spacenix login                 open the browser to sign in"),
            Line::from("  spacenix login --token <pat>   log in with a token"),
            Line::from("  spacenix secret get FOO        print FOO to stdout"),
            Line::from("  spacenix secret set FOO        read value from stdin"),
            Line::from("  spacenix secret list           list secret names"),
            Line::from("  spacenix service start         run the local HTTP API"),
            Line::from("  spacenix sync add <id>         enable syncing for a file"),
            Line::from("  spacenix sync status           show current selection"),
        ];
        let p = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(" Help "))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, area);
    }
}

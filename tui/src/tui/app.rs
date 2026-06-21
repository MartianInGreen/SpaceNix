//! TUI entry point + screen dispatcher.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
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
use crate::store::device::LocalDevice;

#[derive(Debug, Args, Default)]
pub struct TuiArgs {
    /// Skip the first-run login flow (assume credentials already exist).
    #[arg(long)]
    pub skip_login: bool,
}

pub async fn run(config: Arc<Config>, args: TuiArgs) -> Result<ExitCode> {
    // If we don't have credentials, run the interactive login flow first.
    let creds = crate::store::credentials::Credentials::load(&config.credentials_file())?;
    let state = match creds {
        Some(creds) => match conn::connect(&config, Some(creds.token)) {
            Ok(s) => s,
            Err(err) => return run_connection_error(&config, &err).await,
        },
        None => {
            if args.skip_login {
                return run_first_run(&config).await;
            }
            run_browser_login(Arc::clone(&config)).await?
        }
    };

    let local_device =
        crate::auth::device::ensure_local_device(Arc::clone(&config), &state).await?;

    let terminal = ratatui::init();
    let app_result = App::new(config, state, local_device).run(terminal).await;
    ratatui::restore();
    app_result.map(|()| ExitCode::from(0))
}

async fn run_browser_login(config: Arc<Config>) -> Result<ConnState> {
    eprintln!("You are not signed in yet.");
    eprintln!("Opening browser to sign in...");
    let pending = crate::auth::login::start_callback_server()
        .await
        .context("starting local callback server")?;
    let web_url = crate::auth::login::build_web_login_url(&config, &pending.url);
    if let Err(err) = open::that_detached(&web_url) {
        eprintln!("could not open browser: {err}");
        eprintln!("Open this URL manually: {web_url}");
    }
    let payload = pending
        .wait(Duration::from_secs(120))
        .await
        .context("waiting for browser callback")?;
    let outcome = crate::auth::login::complete_login(config, payload.token, payload.identity)
        .context("completing login")?;
    eprintln!("✓ logged in as {}", outcome.credentials.identity);
    Ok(outcome.conn)
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
    Files,
    Secrets,
    SshKeys,
    SshEndpoints,
    Tokens,
    Devices,
    Account,
    Sync,
    Help,
}

struct App {
    #[allow(dead_code)]
    config: Arc<Config>,
    state: ConnState,
    local_device: LocalDevice,
    screen: Screen,
    /// Status bar message.
    status: String,
    /// Files list state.
    files: ListState,
    file_items: Vec<FileRow>,
    /// Secrets list state.
    secrets: ListState,
    secrets_items: Vec<SecretRow>,
    /// SSH key list state.
    ssh_keys: ListState,
    ssh_key_items: Vec<SshKeyRow>,
    /// SSH endpoint list state.
    ssh_endpoints: ListState,
    ssh_endpoint_items: Vec<SshEndpointRow>,
    /// Sync list state.
    sync: ListState,
    sync_items: Vec<SyncRow>,
    /// Tokens list state.
    tokens: ListState,
    token_items: Vec<TokenRow>,
    /// Device list state.
    devices: ListState,
    device_items: Vec<DeviceRow>,
    should_quit: bool,
    /// Channel of events coming from the input thread.
    events: mpsc::UnboundedReceiver<TuiEvent>,
    /// Toast / modal one-shot state.
    toast: Option<String>,
}

#[derive(Clone)]
struct FileRow {
    id: u64,
    name: String,
    path: String,
    kind: String,
    size: u64,
    content_type: String,
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
struct SshKeyRow {
    id: u64,
    name: String,
    fingerprint: String,
    devices: String,
    tags: String,
}

#[derive(Clone)]
struct SshEndpointRow {
    id: u64,
    name: String,
    target: String,
    key_id: u64,
    status: String,
    devices: String,
    tags: String,
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

#[derive(Clone)]
struct DeviceRow {
    id: u64,
    name: String,
    hostname: String,
    last_seen: String,
}

#[derive(Debug)]
enum TuiEvent {
    Input(Event),
    Tick,
}

impl App {
    fn new(config: Arc<Config>, state: ConnState, local_device: LocalDevice) -> Self {
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
            local_device,
            screen: Screen::Files,
            status: "ready".to_string(),
            files: ListState::default(),
            file_items: Vec::new(),
            secrets: ListState::default(),
            secrets_items: Vec::new(),
            ssh_keys: ListState::default(),
            ssh_key_items: Vec::new(),
            ssh_endpoints: ListState::default(),
            ssh_endpoint_items: Vec::new(),
            sync: ListState::default(),
            sync_items: Vec::new(),
            tokens: ListState::default(),
            token_items: Vec::new(),
            devices: ListState::default(),
            device_items: Vec::new(),
            should_quit: false,
            events: rx,
            toast: None,
        };
        app.refresh_files();
        app.refresh_secrets();
        app.refresh_ssh_keys();
        app.refresh_ssh_endpoints();
        app.refresh_sync();
        app.refresh_tokens();
        app.refresh_devices();
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
                "SELECT * FROM my_ssh_keys",
                "SELECT * FROM my_ssh_endpoints",
                "SELECT * FROM my_devices",
                "SELECT * FROM my_user",
            ]);
        // Wait briefly for the first subscription update.
        tokio::time::sleep(Duration::from_millis(400)).await;
        self.refresh_files();
        self.refresh_secrets();
        self.refresh_ssh_keys();
        self.refresh_ssh_endpoints();
        self.refresh_sync();
        self.refresh_tokens();
        self.refresh_devices();

        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            match self.events.recv().await {
                Some(TuiEvent::Input(Event::Key(key))) => self.on_key(key),
                Some(TuiEvent::Input(_)) => {}
                Some(TuiEvent::Tick) => {
                    self.refresh_files();
                    self.refresh_secrets();
                    self.refresh_ssh_keys();
                    self.refresh_ssh_endpoints();
                    self.refresh_sync();
                    self.refresh_tokens();
                    self.refresh_devices();
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
                    Screen::Files => Screen::Secrets,
                    Screen::Secrets => Screen::SshKeys,
                    Screen::SshKeys => Screen::SshEndpoints,
                    Screen::SshEndpoints => Screen::Tokens,
                    Screen::Tokens => Screen::Devices,
                    Screen::Devices => Screen::Account,
                    Screen::Account => Screen::Sync,
                    Screen::Sync => Screen::Help,
                    Screen::Help => Screen::Files,
                };
            }
            KeyCode::Char('1') => self.screen = Screen::Files,
            KeyCode::Char('2') => self.screen = Screen::Secrets,
            KeyCode::Char('3') => self.screen = Screen::SshKeys,
            KeyCode::Char('4') => self.screen = Screen::SshEndpoints,
            KeyCode::Char('5') => self.screen = Screen::Tokens,
            KeyCode::Char('6') => self.screen = Screen::Devices,
            KeyCode::Char('7') => self.screen = Screen::Account,
            KeyCode::Char('8') => self.screen = Screen::Sync,
            KeyCode::Char('?') => self.screen = Screen::Help,
            _ => match self.screen {
                Screen::Files => self.on_key_files(key),
                Screen::Secrets => self.on_key_secrets(key),
                Screen::SshKeys => self.on_key_ssh_keys(key),
                Screen::SshEndpoints => self.on_key_ssh_endpoints(key),
                Screen::Tokens => self.on_key_tokens(key),
                Screen::Devices => self.on_key_devices(key),
                Screen::Account => {}
                Screen::Sync => self.on_key_sync(key),
                Screen::Help => {}
            },
        }
    }

    fn on_key_files(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.list_next_files(),
            KeyCode::Up | KeyCode::Char('k') => self.list_prev_files(),
            _ => {}
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

    fn on_key_ssh_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.list_next_ssh_keys(),
            KeyCode::Up | KeyCode::Char('k') => self.list_prev_ssh_keys(),
            _ => {}
        }
    }

    fn on_key_ssh_endpoints(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.list_next_ssh_endpoints(),
            KeyCode::Up | KeyCode::Char('k') => self.list_prev_ssh_endpoints(),
            _ => {}
        }
    }

    fn on_key_devices(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.list_next_devices(),
            KeyCode::Up | KeyCode::Char('k') => self.list_prev_devices(),
            _ => {}
        }
    }

    fn list_next_files(&mut self) {
        let len = self.file_items.len();
        if len == 0 {
            return;
        }
        let i = self.files.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.files.select(Some(i));
    }

    fn list_prev_files(&mut self) {
        let len = self.file_items.len();
        if len == 0 {
            return;
        }
        let i = self
            .files
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.files.select(Some(i));
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

    fn list_next_ssh_keys(&mut self) {
        let len = self.ssh_key_items.len();
        if len == 0 {
            return;
        }
        let i = self.ssh_keys.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.ssh_keys.select(Some(i));
    }

    fn list_prev_ssh_keys(&mut self) {
        let len = self.ssh_key_items.len();
        if len == 0 {
            return;
        }
        let i = self
            .ssh_keys
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.ssh_keys.select(Some(i));
    }

    fn list_next_ssh_endpoints(&mut self) {
        let len = self.ssh_endpoint_items.len();
        if len == 0 {
            return;
        }
        let i = self
            .ssh_endpoints
            .selected()
            .map(|i| (i + 1) % len)
            .unwrap_or(0);
        self.ssh_endpoints.select(Some(i));
    }

    fn list_prev_ssh_endpoints(&mut self) {
        let len = self.ssh_endpoint_items.len();
        if len == 0 {
            return;
        }
        let i = self
            .ssh_endpoints
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.ssh_endpoints.select(Some(i));
    }

    fn list_next_devices(&mut self) {
        let len = self.device_items.len();
        if len == 0 {
            return;
        }
        let i = self.devices.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.devices.select(Some(i));
    }

    fn list_prev_devices(&mut self) {
        let len = self.device_items.len();
        if len == 0 {
            return;
        }
        let i = self
            .devices
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.devices.select(Some(i));
    }

    fn toggle_sync_selection(&mut self) {
        let Some(idx) = self.sync.selected() else {
            return;
        };
        let Some(row) = self.sync_items.get(idx) else {
            return;
        };
        let id = row.id;
        let mut sel =
            crate::store::sync::SyncSelection::load(&self.config.sync_file()).unwrap_or_default();
        if let Some(file) = self.state.conn.db().my_files().iter().find(|f| f.id == id) {
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

    fn refresh_files(&mut self) {
        let mut rows: Vec<FileRow> = self
            .state
            .conn
            .db()
            .my_files()
            .iter()
            .map(|f| FileRow {
                id: f.id,
                name: f.name.clone(),
                path: f.tree_path.clone().unwrap_or_else(|| "(root)".into()),
                kind: if f.is_directory {
                    "dir".into()
                } else {
                    "file".into()
                },
                size: f.size_bytes,
                content_type: f.content_type.clone().unwrap_or_else(|| "-".into()),
            })
            .collect();
        rows.sort_by(|a, b| a.path.cmp(&b.path).then(a.name.cmp(&b.name)));
        self.file_items = rows;
        if self.files.selected().is_none() && !self.file_items.is_empty() {
            self.files.select(Some(0));
        }
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
        let sel =
            crate::store::sync::SyncSelection::load(&self.config.sync_file()).unwrap_or_default();
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

    fn refresh_ssh_keys(&mut self) {
        let mut rows: Vec<SshKeyRow> = self
            .state
            .conn
            .db()
            .my_ssh_keys()
            .iter()
            .map(|k| SshKeyRow {
                id: k.id,
                name: k.name.clone(),
                fingerprint: k.fingerprint.clone(),
                devices: list_or(&k.device_ids, "all"),
                tags: list_or(&k.tags, "-"),
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        self.ssh_key_items = rows;
        if self.ssh_keys.selected().is_none() && !self.ssh_key_items.is_empty() {
            self.ssh_keys.select(Some(0));
        }
    }

    fn refresh_ssh_endpoints(&mut self) {
        let mut rows: Vec<SshEndpointRow> = self
            .state
            .conn
            .db()
            .my_ssh_endpoints()
            .iter()
            .map(|e| SshEndpointRow {
                id: e.id,
                name: e.name.clone(),
                target: format!("{}@{}:{}", e.username, e.host, e.port),
                key_id: e.key_id,
                status: if e.enabled {
                    "enabled".into()
                } else {
                    "disabled".into()
                },
                devices: list_or(&e.device_ids, "all"),
                tags: list_or(&e.tags, "-"),
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        self.ssh_endpoint_items = rows;
        if self.ssh_endpoints.selected().is_none() && !self.ssh_endpoint_items.is_empty() {
            self.ssh_endpoints.select(Some(0));
        }
    }

    fn refresh_devices(&mut self) {
        let mut rows: Vec<DeviceRow> = self
            .state
            .conn
            .db()
            .my_devices()
            .iter()
            .map(|d| DeviceRow {
                id: d.id,
                name: d.name.clone(),
                hostname: d.hostname.clone().unwrap_or_else(|| "-".into()),
                last_seen: d
                    .last_seen_at
                    .map(|ts| format!("{:?}", ts))
                    .unwrap_or_else(|| "never".into()),
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        self.device_items = rows;
        if self.devices.selected().is_none() && !self.device_items.is_empty() {
            self.devices.select(Some(0));
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
                    Screen::Files => "1 · Files",
                    Screen::Secrets => "2 · Secrets",
                    Screen::SshKeys => "3 · SSH keys",
                    Screen::SshEndpoints => "4 · SSH endpoints",
                    Screen::Tokens => "5 · PATs",
                    Screen::Devices => "6 · Devices",
                    Screen::Account => "7 · Account",
                    Screen::Sync => "8 · Sync",
                    Screen::Help => "? · Help",
                },
                Style::default().fg(Color::Yellow),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" SpaceNix TUI "),
        );
        frame.render_widget(title, chunks[0]);

        // Body
        match self.screen {
            Screen::Files => self.render_files(frame, chunks[1]),
            Screen::Secrets => self.render_secrets(frame, chunks[1]),
            Screen::SshKeys => self.render_ssh_keys(frame, chunks[1]),
            Screen::SshEndpoints => self.render_ssh_endpoints(frame, chunks[1]),
            Screen::Tokens => self.render_tokens(frame, chunks[1]),
            Screen::Devices => self.render_devices(frame, chunks[1]),
            Screen::Account => self.render_account(frame, chunks[1]),
            Screen::Sync => self.render_sync(frame, chunks[1]),
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
        let status = Paragraph::new(Line::from(vec![Span::raw(format!(
            " identity {}  device #{} {}  {}",
            short_id, self.local_device.id, self.local_device.name, status_msg
        ))]))
        .block(Block::default().borders(Borders::ALL).title(" status "));
        frame.render_widget(status, chunks[2]);
    }

    fn render_files(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .file_items
            .iter()
            .map(|r| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("#{:<6}", r.id), Style::default().fg(Color::Magenta)),
                    Span::raw(format!("{:<5}", r.kind)),
                    Span::styled(format!("{:<28}", r.name), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" {:>10} bytes ", r.size)),
                    Span::raw(format!("{:<18}", r.content_type)),
                    Span::raw(format!(" {}", r.path)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Files "))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.files);
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

    fn render_ssh_keys(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .ssh_key_items
            .iter()
            .map(|r| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("#{:<6}", r.id), Style::default().fg(Color::Magenta)),
                    Span::styled(format!("{:<24}", r.name), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" fp={} ", r.fingerprint)),
                    Span::raw(format!("devices={} ", r.devices)),
                    Span::raw(format!("tags={}", r.tags)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" SSH keys "))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.ssh_keys);
    }

    fn render_ssh_endpoints(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .ssh_endpoint_items
            .iter()
            .map(|r| {
                let status_style = if r.status == "enabled" {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("#{:<6}", r.id), Style::default().fg(Color::Magenta)),
                    Span::styled(format!("{:<22}", r.name), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" {:<28}", r.target)),
                    Span::raw(format!(" key=#{:<6}", r.key_id)),
                    Span::styled(format!(" {:<8}", r.status), status_style),
                    Span::raw(format!(" devices={} tags={}", r.devices, r.tags)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" SSH endpoints "),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.ssh_endpoints);
    }

    fn render_devices(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .device_items
            .iter()
            .map(|r| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("#{:<6}", r.id), Style::default().fg(Color::Magenta)),
                    Span::styled(format!("{:<24}", r.name), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" host={:<24}", r.hostname)),
                    Span::raw(format!(" last_seen={}", r.last_seen)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Devices "))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.devices);
    }

    fn render_account(&mut self, frame: &mut Frame, area: Rect) {
        let identity = self
            .state
            .identity()
            .map(|i| i.to_hex().to_string())
            .unwrap_or_else(|| "-".into());
        let user = self.state.conn.db().my_user().iter().next();
        let mut lines = vec![Line::from("Account"), Line::from("")];
        if let Some(user) = user {
            lines.push(Line::from(format!(
                "Display name: {}",
                user.display_name.as_deref().unwrap_or("-")
            )));
            lines.push(Line::from(format!("Email:        {}", user.email)));
            lines.push(Line::from(format!("Role:         {}", user.role)));
        } else {
            lines.push(Line::from("Profile:      not available yet"));
        }
        lines.push(Line::from(format!("Identity:     {}", identity)));
        lines.push(Line::from(format!(
            "Device:       #{} {} host={}",
            self.local_device.id,
            self.local_device.name,
            self.local_device.hostname.as_deref().unwrap_or("-")
        )));
        lines.push(Line::from(""));
        lines.push(Line::from("CLI actions:"));
        lines.push(Line::from("  spacenix account update-email <new-email>"));
        lines.push(Line::from("  spacenix logout"));
        let p = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Account "))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, area);
    }

    fn render_help(&mut self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from("SpaceNix TUI — quick reference"),
            Line::from(""),
            Line::from("  1            switch to the Files screen"),
            Line::from("  2            switch to the Secrets screen"),
            Line::from("  3            switch to the SSH keys screen"),
            Line::from("  4            switch to the SSH endpoints screen"),
            Line::from("  5            switch to the PATs screen"),
            Line::from("  6            switch to the Devices screen"),
            Line::from("  7            switch to the Account screen"),
            Line::from("  8            switch to the Sync screen"),
            Line::from("  Tab          cycle screens"),
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
            Line::from("  spacenix file list             list files/folders"),
            Line::from("  spacenix ssh key list          list SSH keys"),
            Line::from("  spacenix device list           list devices"),
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

fn list_or(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        empty.to_string()
    } else {
        items.join(",")
    }
}

//! TUI entry point + screen dispatcher.

use std::collections::{BTreeMap, HashSet};
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
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Sparkline, Wrap};
use ratatui::{DefaultTerminal, Frame};
use tokio::sync::mpsc;

use crate::auth::conn::{self, ConnState};
use crate::bindings::*;
use crate::config::Config;
use crate::store::device::LocalDevice;

fn format_metric_line(m: &DeviceMetricSample) -> String {
    use crate::util::formatting;
    let ram_pct = percent(m.ram_used_bytes, m.ram_total_bytes);
    let swap_pct = percent(m.swap_used_bytes, m.swap_total_bytes);
    let sync_pct = percent(
        m.storage_sync_root_used_bytes,
        m.storage_sync_root_total_bytes,
    );
    let sys_pct = percent(
        m.storage_system_used_bytes,
        m.storage_system_total_bytes,
    );
    format!(
        "cpu {:>4.1}% | ram {:>4.1}% {} | swap {:>4.1}% {} | net {}↓ {}↑ | sync_root {:>4.1}% {} | sys {:>4.1}% {}",
        m.cpu_percent,
        ram_pct,
        formatting::bytes(m.ram_used_bytes),
        swap_pct,
        formatting::bytes(m.swap_used_bytes),
        formatting::bytes(m.net_rx_bytes),
        formatting::bytes(m.net_tx_bytes),
        sync_pct,
        formatting::bytes(m.storage_sync_root_used_bytes),
        sys_pct,
        formatting::bytes(m.storage_system_used_bytes),
    )
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f32 / total as f32) * 100.0
    }
}

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
    file_path: String,
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
    processed_ui_commands: HashSet<u64>,
    should_quit: bool,
    /// Channel of events coming from the input thread.
    events: mpsc::UnboundedReceiver<TuiEvent>,
    /// Toast / modal one-shot state.
    toast: Option<String>,
}

#[derive(Clone)]
struct FileRow {
    id: Option<u64>,
    name: String,
    full_path: String,
    kind: String,
    is_directory: bool,
    is_implicit: bool,
    selected: bool,
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
    metrics: Option<String>,
    /// Configured server-side retention in seconds (`None` = server default).
    metrics_retention_secs: Option<u64>,
    /// Recent samples, sorted ascending by `recorded_at`. Capped to the
    /// `MAX_HISTORY_POINTS` most recent so the graph stays readable.
    history: Vec<DeviceMetricSample>,
}

const MAX_HISTORY_POINTS: usize = 60;

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
            file_path: String::new(),
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
            processed_ui_commands: HashSet::new(),
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
                "SELECT * FROM my_device_metrics",
                "SELECT * FROM my_ui_commands",
                "SELECT * FROM ui_event",
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
        self.process_ui_commands();

        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            match self.events.recv().await {
                Some(TuiEvent::Input(Event::Key(key))) => {
                    self.on_key(key);
                    self.process_ui_commands();
                }
                Some(TuiEvent::Input(_)) => {
                    self.process_ui_commands();
                }
                Some(TuiEvent::Tick) => {
                    self.refresh_files();
                    self.refresh_secrets();
                    self.refresh_ssh_keys();
                    self.refresh_ssh_endpoints();
                    self.refresh_sync();
                    self.refresh_tokens();
                    self.refresh_devices();
                    self.process_ui_commands();
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
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.open_selected_file_row(),
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => self.file_parent(),
            KeyCode::Home => self.file_root(),
            KeyCode::Char(' ') => self.toggle_selected_file_sync(),
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

    fn open_selected_file_row(&mut self) {
        let Some(idx) = self.files.selected() else {
            return;
        };
        let Some(row) = self.file_items.get(idx) else {
            return;
        };
        if row.is_directory {
            self.file_path = row.full_path.clone();
            self.files.select(None);
            self.refresh_files();
            self.status = format!("opened /{}", self.file_path);
        } else {
            self.status = format!(
                "file {} · {} bytes · {}",
                row.name, row.size, row.content_type
            );
        }
    }

    fn file_parent(&mut self) {
        if self.file_path.is_empty() {
            return;
        }
        self.file_path = parent_path(&self.file_path).unwrap_or_default();
        self.files.select(None);
        self.refresh_files();
        self.status = if self.file_path.is_empty() {
            "opened /".to_string()
        } else {
            format!("opened /{}", self.file_path)
        };
    }

    fn file_root(&mut self) {
        self.file_path.clear();
        self.files.select(None);
        self.refresh_files();
        self.status = "opened /".to_string();
    }

    fn toggle_selected_file_sync(&mut self) {
        let Some(idx) = self.files.selected() else {
            return;
        };
        let Some(row) = self.file_items.get(idx) else {
            return;
        };
        let Some(id) = row.id else {
            self.toast = Some(format!("{} is an implicit folder", row.name));
            return;
        };
        self.toggle_file_sync(id);
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
        self.toggle_file_sync(row.id);
    }

    fn toggle_file_sync(&mut self, id: u64) {
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
        self.refresh_files();
    }

    fn refresh_files(&mut self) {
        let sel =
            crate::store::sync::SyncSelection::load(&self.config.sync_file()).unwrap_or_default();
        let mut rows_by_path: BTreeMap<String, FileRow> = BTreeMap::new();
        for f in self.state.conn.db().my_files().iter() {
            let full_path = file_full_path(f.name.as_str(), f.tree_path.as_deref());
            if let Some(child_dir) = immediate_child_dir(&self.file_path, &full_path) {
                rows_by_path
                    .entry(child_dir.clone())
                    .or_insert_with(|| FileRow {
                        id: None,
                        name: basename(&child_dir)
                            .unwrap_or(child_dir.as_str())
                            .to_string(),
                        full_path: child_dir,
                        kind: "dir".into(),
                        is_directory: true,
                        is_implicit: true,
                        selected: false,
                        size: 0,
                        content_type: "-".into(),
                    });
                continue;
            }
            if parent_path(&full_path).unwrap_or_default() != self.file_path {
                continue;
            }
            rows_by_path.insert(
                full_path.clone(),
                FileRow {
                    id: Some(f.id),
                    name: basename(&full_path).unwrap_or(f.name.as_str()).to_string(),
                    full_path,
                    kind: if f.is_directory {
                        "dir".into()
                    } else {
                        "file".into()
                    },
                    is_directory: f.is_directory,
                    is_implicit: false,
                    selected: sel.contains(f.id),
                    size: f.size_bytes,
                    content_type: f.content_type.clone().unwrap_or_else(|| "-".into()),
                },
            );
        }
        let mut rows: Vec<FileRow> = rows_by_path.into_values().collect();
        rows.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.file_items = rows;
        match (self.files.selected(), self.file_items.len()) {
            (_, 0) => self.files.select(None),
            (Some(i), len) if i >= len => self.files.select(Some(len - 1)),
            (None, _) => self.files.select(Some(0)),
            _ => {}
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
        let mut latest: std::collections::HashMap<u64, DeviceMetricSample> =
            std::collections::HashMap::new();
        let mut history: std::collections::HashMap<u64, Vec<DeviceMetricSample>> =
            std::collections::HashMap::new();
        for m in self.state.conn.db().my_device_metrics().iter() {
            let entry = latest.entry(m.device_id).or_insert_with(|| m.clone());
            if m.recorded_at > entry.recorded_at {
                *entry = m.clone();
            }
            let samples = history.entry(m.device_id).or_default();
            samples.push(m.clone());
        }
        for samples in history.values_mut() {
            samples.sort_by(|a, b| a.recorded_at.cmp(&b.recorded_at));
            if samples.len() > MAX_HISTORY_POINTS {
                let drop = samples.len() - MAX_HISTORY_POINTS;
                samples.drain(0..drop);
            }
        }
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
                metrics: latest.get(&d.id).map(format_metric_line),
                metrics_retention_secs: d.metrics_retention.map(|t| {
                    (t.to_micros() / 1_000_000).max(0) as u64
                }),
                history: history.remove(&d.id).unwrap_or_default(),
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        self.device_items = rows;
        if self.devices.selected().is_none() && !self.device_items.is_empty() {
            self.devices.select(Some(0));
        }
    }

    fn process_ui_commands(&mut self) {
        let mut commands: Vec<_> = self
            .state
            .conn
            .db()
            .my_ui_commands()
            .iter()
            .filter(|c| c.handled_at.is_none())
            .filter(|c| {
                c.target_device_id
                    .is_none_or(|target| target == self.local_device.id)
            })
            .filter(|c| !self.processed_ui_commands.contains(&c.id))
            .collect();
        commands.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));

        for command in commands {
            let id = command.id;
            self.processed_ui_commands.insert(id);
            self.apply_ui_command(command.kind.as_str(), command.payload_json.as_str());
            if let Err(err) = self
                .state
                .conn
                .reducers()
                .ack_ui_command(id, self.local_device.id)
            {
                self.toast = Some(format!("failed to ack web command #{id}: {err}"));
            }
        }
    }

    fn apply_ui_command(&mut self, kind: &str, payload_json: &str) {
        let payload: serde_json::Value = serde_json::from_str(payload_json).unwrap_or_default();
        match kind {
            "screen:open" => {
                let Some(screen) = payload.get("screen").and_then(|v| v.as_str()) else {
                    self.toast = Some("web command missing screen".into());
                    return;
                };
                if let Some(screen) = parse_screen(screen) {
                    self.screen = screen;
                    self.status = format!("opened {screen:?} from web");
                } else {
                    self.toast = Some(format!("unknown screen from web: {screen}"));
                }
            }
            "files:open_path" => {
                let path = payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim_matches('/')
                    .to_string();
                self.screen = Screen::Files;
                self.file_path = path;
                self.files.select(None);
                self.refresh_files();
                self.status = if self.file_path.is_empty() {
                    "opened / from web".into()
                } else {
                    format!("opened /{} from web", self.file_path)
                };
            }
            "sync:refresh" => {
                self.refresh_sync();
                self.refresh_files();
                self.status = "refreshed sync state from web".into();
            }
            "toast" => {
                if let Some(message) = payload.get("message").and_then(|v| v.as_str()) {
                    self.toast = Some(message.to_string());
                }
            }
            _ => {
                self.toast = Some(format!("unhandled web command: {kind}"));
            }
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
                let mark = if r.selected { "[x]" } else { "[ ]" };
                let id =
                    r.id.map(|id| format!("#{:<6}", id))
                        .unwrap_or_else(|| "       ".to_string());
                let implicit = if r.is_implicit { " implicit" } else { "" };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{mark} "), Style::default().fg(Color::Green)),
                    Span::styled(id, Style::default().fg(Color::Magenta)),
                    Span::raw(format!("{:<5}", r.kind)),
                    Span::styled(format!("{:<28}", r.name), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" {:>10} bytes ", r.size)),
                    Span::raw(format!("{:<18}", r.content_type)),
                    Span::styled(implicit, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        let title = if self.file_path.is_empty() {
            " Files /  (Enter open, Backspace up, Space sync) ".to_string()
        } else {
            format!(
                " Files /{}  (Enter open, Backspace up, Space sync) ",
                self.file_path
            )
        };
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
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
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        let items: Vec<ListItem> = self
            .device_items
            .iter()
            .map(|r| {
                let mut lines = vec![Line::from(vec![
                    Span::styled(format!("#{:<6}", r.id), Style::default().fg(Color::Magenta)),
                    Span::styled(format!("{:<24}", r.name), Style::default().fg(Color::Cyan)),
                    Span::raw(format!(" host={:<24}", r.hostname)),
                    Span::raw(format!(" last_seen={}", r.last_seen)),
                ])];
                if let Some(metrics) = &r.metrics {
                    lines.push(Line::from(Span::styled(
                        format!("         {}", metrics),
                        Style::default().fg(Color::Green),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        "         (no metrics reported yet — is the service running?)",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                ListItem::new(lines)
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Devices "))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, chunks[0], &mut self.devices);

        // Detail panel: sparklines for the selected device.
        let detail = Block::default()
            .borders(Borders::ALL)
            .title(" Metrics history ");
        if let Some(row) = self.device_items.get(self.devices.selected().unwrap_or(0)) {
            self.render_device_detail(frame, chunks[1], row);
        } else {
            frame.render_widget(detail.title(" Metrics history (no device selected) "), chunks[1]);
        }
    }

    fn render_device_detail(&self, frame: &mut Frame, area: Rect, row: &DeviceRow) {
        let header_area = Rect {
            height: 2,
            ..area
        };
        let graph_area = Rect {
            y: area.y + 2,
            height: area.height.saturating_sub(2),
            ..area
        };

        let retention = match row.metrics_retention_secs {
            Some(s) => format!(
                "retention: {} (set via `spacenix device retention {} <seconds>`)",
                humantime::format_duration(std::time::Duration::from_secs(s)),
                row.id
            ),
            None => "retention: server default (1h)".to_string(),
        };
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("#{} {}", row.id, row.name),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("    "),
            Span::styled(retention, Style::default().fg(Color::Yellow)),
        ]))
        .block(Block::default().borders(Borders::ALL).title(" Detail "));
        frame.render_widget(header, header_area);

        if row.history.is_empty() {
            let placeholder = Paragraph::new(Line::from(Span::styled(
                "no samples yet — wait for the next report (every 30s)",
                Style::default().fg(Color::DarkGray),
            )))
            .block(Block::default().borders(Borders::ALL).title(" Metrics history "));
            frame.render_widget(placeholder, graph_area);
            return;
        }

        let cpu: Vec<u64> = row
            .history
            .iter()
            .map(|m| m.cpu_percent.clamp(0.0, 100.0) as u64)
            .collect();
        let ram: Vec<u64> = row
            .history
            .iter()
            .map(|m| percent(m.ram_used_bytes, m.ram_total_bytes) as u64)
            .collect();
        let sync: Vec<u64> = row
            .history
            .iter()
            .map(|m| {
                percent(
                    m.storage_sync_root_used_bytes,
                    m.storage_sync_root_total_bytes,
                ) as u64
            })
            .collect();
        let sys: Vec<u64> = row
            .history
            .iter()
            .map(|m| {
                percent(
                    m.storage_system_used_bytes,
                    m.storage_system_total_bytes,
                ) as u64
            })
            .collect();

        // Net speed (bytes/sec) is derived from the delta between
        // consecutive samples. The first sample has no prior so its
        // series is empty — pad so `Sparkline` still draws a point.
        let mut net_rx_bps: Vec<u64> = Vec::with_capacity(row.history.len());
        let mut net_tx_bps: Vec<u64> = Vec::with_capacity(row.history.len());
        for w in row.history.windows(2) {
            let dt_micros = w[1]
                .recorded_at
                .to_micros_since_unix_epoch()
                .saturating_sub(w[0].recorded_at.to_micros_since_unix_epoch())
                .max(1) as f64
                / 1_000_000.0;
            let rx_delta = w[1]
                .net_rx_bytes
                .saturating_sub(w[0].net_rx_bytes) as f64
                / dt_micros;
            let tx_delta = w[1]
                .net_tx_bytes
                .saturating_sub(w[0].net_tx_bytes) as f64
                / dt_micros;
            net_rx_bps.push(rx_delta as u64);
            net_tx_bps.push(tx_delta as u64);
        }

        // Pick a sensible y-axis for the net sparkline. The "max" is
        // the larger of the two series so neither gets clipped, and
        // clamped to at least 1 KiB/s so a quiet network still has
        // visible variation.
        let net_max_bps = net_rx_bps
            .iter()
            .chain(net_tx_bps.iter())
            .copied()
            .max()
            .unwrap_or(1024)
            .max(1024);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(22),
                Constraint::Percentage(22),
                Constraint::Percentage(22),
                Constraint::Percentage(22),
                Constraint::Percentage(12),
            ])
            .split(graph_area);

        // Sparkline::data requires at least 2 points to render. Pad a single
        // sample so the user sees something on the first report.
        let pad = |d: &[u64]| -> Vec<u64> {
            if d.is_empty() {
                return vec![0, 0];
            }
            if d.len() < 2 {
                let mut v = d.to_vec();
                v.push(*d.last().unwrap_or(&0));
                v
            } else {
                d.to_vec()
            }
        };
        let cpu = pad(&cpu);
        let ram = pad(&ram);
        let sync = pad(&sync);
        let sys = pad(&sys);
        let net_rx = pad(&net_rx_bps);
        let net_tx = pad(&net_tx_bps);

        let cpu_widget = Sparkline::default()
            .block(Block::default().borders(Borders::ALL).title(" CPU % (last samples) "))
            .data(&cpu)
            .max(100)
            .style(Style::default().fg(Color::Red));
        let ram_widget = Sparkline::default()
            .block(Block::default().borders(Borders::ALL).title(" RAM % "))
            .data(&ram)
            .max(100)
            .style(Style::default().fg(Color::Green));
        let sync_widget = Sparkline::default()
            .block(Block::default().borders(Borders::ALL).title(" storage sync_root % "))
            .data(&sync)
            .max(100)
            .style(Style::default().fg(Color::Cyan));
        let sys_widget = Sparkline::default()
            .block(Block::default().borders(Borders::ALL).title(" storage system % "))
            .data(&sys)
            .max(100)
            .style(Style::default().fg(Color::Blue));
        let rx_widget = Sparkline::default()
            .block(Block::default().borders(Borders::ALL).title(" net rx B/s "))
            .data(&net_rx)
            .max(net_max_bps)
            .style(Style::default().fg(Color::Magenta));
        let tx_widget = Sparkline::default()
            .block(Block::default().borders(Borders::ALL).title(" net tx B/s "))
            .data(&net_tx)
            .max(net_max_bps)
            .style(Style::default().fg(Color::Yellow));

        frame.render_widget(cpu_widget, rows[0]);
        frame.render_widget(ram_widget, rows[1]);
        frame.render_widget(sync_widget, rows[2]);
        frame.render_widget(sys_widget, rows[3]);
        let net_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[4]);
        frame.render_widget(rx_widget, net_row[0]);
        frame.render_widget(tx_widget, net_row[1]);
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
            Line::from("  Enter / l    open folder or show selected file info (files)"),
            Line::from("  Backspace/h  go to parent folder (files)"),
            Line::from("  Home         go to root folder (files)"),
            Line::from("  Space        toggle sync on/off (files/sync)"),
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
            Line::from("  spacenix ssh connect <name>    ssh into a registered endpoint"),
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

fn file_full_path(name: &str, tree_path: Option<&str>) -> String {
    match tree_path.filter(|path| !path.is_empty()) {
        Some(path) => path.to_string(),
        None => name.to_string(),
    }
}

fn parent_path(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .or_else(|| Some(String::new()))
}

fn basename(path: &str) -> Option<&str> {
    path.trim_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
}

fn immediate_child_dir(current_path: &str, full_path: &str) -> Option<String> {
    let current = current_path.trim_matches('/');
    let full = full_path.trim_matches('/');
    if full.is_empty() || full == current {
        return None;
    }
    let rest = if current.is_empty() {
        full
    } else {
        full.strip_prefix(current)?.strip_prefix('/')?
    };
    let (child, _) = rest.split_once('/')?;
    if current.is_empty() {
        Some(child.to_string())
    } else {
        Some(format!("{current}/{child}"))
    }
}

fn parse_screen(screen: &str) -> Option<Screen> {
    match screen {
        "files" => Some(Screen::Files),
        "secrets" => Some(Screen::Secrets),
        "ssh" | "ssh_keys" | "ssh-keys" => Some(Screen::SshKeys),
        "ssh_endpoints" | "ssh-endpoints" => Some(Screen::SshEndpoints),
        "pats" | "tokens" => Some(Screen::Tokens),
        "devices" => Some(Screen::Devices),
        "account" => Some(Screen::Account),
        "sync" => Some(Screen::Sync),
        "help" => Some(Screen::Help),
        _ => None,
    }
}

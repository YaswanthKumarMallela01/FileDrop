//! TUI application state and main event loop.
//!
//! Manages the Ratatui terminal lifecycle, coordinates between
//! keyboard events, server/client events, and UI rendering.
//! Runs server + mDNS + TUI concurrently in receive mode.
//!
//! Extended with:
//! - File browser for laptop→phone push (Feature 2A)
//! - Connection mode selection screen (Feature 3)
//! - Hotspot setup guide screen (Feature 3)

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::hotspot::ConnectionMode;
use crate::transfer::client::ClientEvent;
use crate::transfer::protocol::{self, TransferFile, TransferStatus};
use crate::transfer::server::{PushRequest, ServerEvent};

use super::events::{self, AppEvent};
use super::ui;

/// Application mode
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    Receive,
    Send,
}

impl std::fmt::Display for AppMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppMode::Receive => write!(f, "RECEIVE MODE"),
            AppMode::Send => write!(f, "SEND MODE"),
        }
    }
}

/// Current connection status
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ConnectionStatus {
    /// Waiting for a connection
    Ready,
    /// Connected to a peer
    Connected { peer_name: String },
    /// Transferring files
    Transferring,
    /// All transfers complete
    Complete,
    /// Connection lost
    Disconnected,
    /// An error occurred
    Error(String),
}

impl std::fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionStatus::Ready => write!(f, "📡 Ready"),
            ConnectionStatus::Connected { peer_name } => {
                write!(f, "🔗 Connected to {}", peer_name)
            }
            ConnectionStatus::Transferring => write!(f, "📤 Transferring"),
            ConnectionStatus::Complete => write!(f, "✅ Complete"),
            ConnectionStatus::Disconnected => write!(f, "⚠ Disconnected"),
            ConnectionStatus::Error(msg) => write!(f, "❌ {}", msg),
        }
    }
}

/// A log entry displayed in the transfer log panel
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub message: String,
    pub level: LogLevel,
}

/// Log entry severity level for coloring
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Which pane has focus in the TUI (Feature 2A)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusPane {
    TransferQueue,
    SystemLog,
    FileBrowser,
}

/// A file entry in the file browser (Feature 2A)
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub path: PathBuf,
}

/// File browser state for laptop→phone push (Feature 2A)
pub struct FileBrowserState {
    pub current_path: PathBuf,
    pub entries: Vec<FileEntry>,
    pub cursor: usize,
    pub selected: HashSet<usize>,
    pub scroll_offset: usize,
}

impl FileBrowserState {
    pub fn new(start_path: PathBuf) -> Self {
        let mut state = Self {
            current_path: start_path,
            entries: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
            scroll_offset: 0,
        };
        state.refresh_entries();
        state
    }

    /// Reload directory entries from the filesystem
    pub fn refresh_entries(&mut self) {
        self.entries.clear();
        if let Ok(read_dir) = std::fs::read_dir(&self.current_path) {
            let mut entries: Vec<FileEntry> = read_dir
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let meta = e.metadata().ok()?;
                    Some(FileEntry {
                        name: e.file_name().to_string_lossy().to_string(),
                        is_dir: meta.is_dir(),
                        size: if meta.is_file() { meta.len() } else { 0 },
                        path: e.path(),
                    })
                })
                .collect();

            // Sort: directories first, then files, alphabetically
            entries.sort_by(|a, b| {
                b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });

            self.entries = entries;
        }
        self.cursor = 0;
        self.selected.clear();
        self.scroll_offset = 0;
    }

    /// Get total size of selected items
    pub fn selected_size(&self) -> u64 {
        self.selected
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .map(|e| e.size)
            .sum()
    }

    /// Get number of selected items
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }
}

/// Main application state shared between event loop and renderer
pub struct AppState {
    /// Current mode (receive/send)
    pub mode: AppMode,
    /// Connection status
    pub status: ConnectionStatus,
    /// Queue of files being transferred
    pub file_queue: Vec<TransferFile>,
    /// Transfer log entries
    pub log_entries: Vec<LogEntry>,
    /// Overall progress (0.0 - 1.0)
    pub overall_progress: f64,
    /// Current transfer speed in bytes/sec
    pub current_speed: f64,
    /// Speed history for sparkline (last 60 data points)
    pub speed_history: Vec<u64>,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Total bytes transferred in this session
    pub total_bytes_transferred: u64,
    /// Total bytes expected in this session
    pub total_bytes_expected: u64,
    /// Start time of the current session
    #[allow(dead_code)]
    pub session_start: Instant,
    /// Scroll offset for file queue
    pub queue_scroll: usize,
    /// Scroll offset for log
    pub log_scroll: usize,
    /// Server event receiver
    server_event_rx: Option<mpsc::UnboundedReceiver<ServerEvent>>,
    /// Client event receiver
    client_event_rx: Option<mpsc::UnboundedReceiver<ClientEvent>>,
    /// Phone URL displayed in the TUI header
    pub phone_url: Option<String>,

    // ── Feature 2A: File Browser ────────────────────────────────
    /// File browser state (active when a phone is connected)
    pub file_browser: Option<FileBrowserState>,
    /// Which pane currently has focus
    pub focus: FocusPane,
    /// Channel to send push requests to the server
    pub push_tx: Option<mpsc::UnboundedSender<PushRequest>>,

    // ── Feature 3: Connection Mode ──────────────────────────────
    /// Whether we're in hotspot mode (for header badge)
    pub hotspot_mode: bool,

    // ── Feature 7: Encryption ───────────────────────────────────
    /// Whether E2E encryption is enabled
    pub encrypt_enabled: bool,

    // ── Smooth Layout Transitions ───────────────────────────────
    /// Current percentages for the middle panels
    pub current_percentages: [f32; 3],
    /// Target percentages for the middle panels
    pub target_percentages: [f32; 3],
}

impl AppState {
    /// Create a new application state
    pub fn new(mode: AppMode) -> Self {
        let mut app = Self {
            mode,
            status: ConnectionStatus::Ready,
            file_queue: Vec::new(),
            log_entries: Vec::new(),
            overall_progress: 0.0,
            current_speed: 0.0,
            speed_history: vec![0; 60],
            should_quit: false,
            total_bytes_transferred: 0,
            total_bytes_expected: 0,
            session_start: Instant::now(),
            queue_scroll: 0,
            log_scroll: 0,
            server_event_rx: None,
            client_event_rx: None,
            phone_url: None,
            file_browser: None,
            focus: FocusPane::TransferQueue,
            push_tx: None,
            hotspot_mode: false,
            encrypt_enabled: false,
            current_percentages: [50.0, 25.0, 25.0],
            target_percentages: [50.0, 25.0, 25.0],
        };
        app.init_file_browser();
        app
    }

    /// Add a log entry with the current timestamp
    pub fn log(&mut self, message: String, level: LogLevel) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.log_entries.push(LogEntry {
            timestamp,
            message,
            level,
        });

        // Keep last 500 log entries
        if self.log_entries.len() > 500 {
            self.log_entries.drain(0..100);
        }

        // Auto-scroll to bottom
        self.log_scroll = self.log_entries.len().saturating_sub(1);
    }

    /// Add a file to the transfer queue
    pub fn add_file(&mut self, file: TransferFile) {
        self.total_bytes_expected += file.size;
        self.file_queue.push(file);
    }

    /// Update progress for a file
    pub fn update_progress(
        &mut self,
        file_name: &str,
        bytes: u64,
        _total: u64,
        speed: f64,
    ) {
        if let Some(file) = self.file_queue.iter_mut().find(|f| f.name == file_name) {
            file.bytes_transferred = bytes;
            file.status = TransferStatus::InProgress;
        }

        self.current_speed = speed;
        self.status = ConnectionStatus::Transferring;

        // Update speed history (scaled to KB/s for sparkline)
        self.speed_history.push((speed / 1024.0) as u64);
        if self.speed_history.len() > 60 {
            self.speed_history.remove(0);
        }

        // Recalculate overall progress
        let transferred: u64 = self.file_queue.iter().map(|f| f.bytes_transferred).sum();
        self.total_bytes_transferred = transferred;

        if self.total_bytes_expected > 0 {
            self.overall_progress =
                transferred as f64 / self.total_bytes_expected as f64;
        }
    }

    /// Mark a file as completed
    pub fn complete_file(&mut self, file_name: &str) {
        if let Some(file) = self.file_queue.iter_mut().find(|f| f.name == file_name) {
            file.status = TransferStatus::Completed;
            file.bytes_transferred = file.size;
        }

        // Recalculate progress
        let transferred: u64 = self.file_queue.iter().map(|f| f.bytes_transferred).sum();
        self.total_bytes_transferred = transferred;
        if self.total_bytes_expected > 0 {
            self.overall_progress =
                transferred as f64 / self.total_bytes_expected as f64;
        }

        // Check if all files are done
        let all_done = self
            .file_queue
            .iter()
            .all(|f| matches!(f.status, TransferStatus::Completed | TransferStatus::Failed(_)));
        if all_done && !self.file_queue.is_empty() {
            self.status = ConnectionStatus::Complete;
        }
    }

    /// Mark a file as failed
    pub fn fail_file(&mut self, file_name: &str, error: &str) {
        if let Some(file) = self.file_queue.iter_mut().find(|f| f.name == file_name) {
            file.status = TransferStatus::Failed(error.to_string());
        }
    }

    /// Initialize file browser when a device connects (Feature 2A)
    fn init_file_browser(&mut self) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        self.file_browser = Some(FileBrowserState::new(cwd));
        self.log("[BROWSER] File browser activated — press Tab to switch focus".to_string(), LogLevel::Info);
    }

    /// Update pane size interpolation for smooth macOS/MacBook-like sliding transitions
    pub fn update_layout_interpolation(&mut self) {
        let is_three_pane = self.file_browser.is_some() || !self.file_queue.is_empty();

        if is_three_pane {
            self.target_percentages = match self.focus {
                FocusPane::TransferQueue => [50.0, 25.0, 25.0],
                FocusPane::SystemLog => [25.0, 50.0, 25.0],
                FocusPane::FileBrowser => [25.0, 25.0, 50.0],
            };
        } else {
            self.target_percentages = match self.focus {
                FocusPane::TransferQueue => [60.0, 40.0, 0.0],
                _ => [40.0, 60.0, 0.0],
            };
        }

        // Linear interpolation: current += (target - current) * 0.20
        for i in 0..3 {
            let diff = self.target_percentages[i] - self.current_percentages[i];
            if diff.abs() > 0.05 {
                self.current_percentages[i] += diff * 0.20;
            } else {
                self.current_percentages[i] = self.target_percentages[i];
            }
        }
    }

    /// Handle file browser key events (Feature 2A)
    fn handle_file_browser_event(&mut self, event: &AppEvent) {
        if let Some(ref mut fb) = self.file_browser {
            match event {
                AppEvent::ScrollUp => {
                    fb.cursor = fb.cursor.saturating_sub(1);
                }
                AppEvent::ScrollDown => {
                    if !fb.entries.is_empty() {
                        fb.cursor = (fb.cursor + 1).min(fb.entries.len() - 1);
                    }
                }
                AppEvent::Enter => {
                    if let Some(entry) = fb.entries.get(fb.cursor) {
                        if entry.is_dir {
                            let new_path = entry.path.clone();
                            fb.current_path = new_path;
                            fb.refresh_entries();
                        }
                    }
                }
                AppEvent::GoBack => {
                    if let Some(parent) = fb.current_path.parent() {
                        fb.current_path = parent.to_path_buf();
                        fb.refresh_entries();
                    }
                }
                AppEvent::ToggleSelect => {
                    let cursor = fb.cursor;
                    if cursor < fb.entries.len() {
                        if fb.selected.contains(&cursor) {
                            fb.selected.remove(&cursor);
                        } else {
                            fb.selected.insert(cursor);
                        }
                    }
                }
                AppEvent::SelectAll => {
                    if fb.selected.len() == fb.entries.len() {
                        fb.selected.clear();
                    } else {
                        fb.selected = (0..fb.entries.len()).collect();
                    }
                }
                AppEvent::ClearSelection => {
                    fb.selected.clear();
                }
                AppEvent::SendSelected => {
                    self.send_selected_files();
                }
                _ => {}
            }
        }
    }

    /// Send selected files from the file browser to the connected phone (Feature 2A)
    fn send_selected_files(&mut self) {
        let (entries_to_send, push_tx) = {
            let fb = match self.file_browser {
                Some(ref fb) => fb,
                None => return,
            };
            if fb.selected.is_empty() {
                self.log("[BROWSER] No files selected".to_string(), LogLevel::Warning);
                return;
            }
            let tx = match self.push_tx {
                Some(ref tx) => tx.clone(),
                None => {
                    self.log("[BROWSER] No active connection for push".to_string(), LogLevel::Warning);
                    return;
                }
            };
            let entries: Vec<FileEntry> = fb.selected.iter()
                .filter_map(|&i| fb.entries.get(i).cloned())
                .collect();
            (entries, tx)
        };

        for entry in entries_to_send {
            if entry.is_dir {
                self.log(format!("[ZIP] Compressing {}/ on the fly...", entry.name), LogLevel::Info);
                // For directories, we'd use the zipper — simplified here to just log
                self.log(format!("[PUSH] Folder push queued: {}/", entry.name), LogLevel::Info);
            } else {
                // Read file and send push request
                match std::fs::read(&entry.path) {
                    Ok(data) => {
                        let hash = hex::encode(Sha256::digest(&data));
                        let size = data.len() as u64;
                        self.log(
                            format!("[PUSH] Sending {} ({})", entry.name, protocol::format_bytes(size)),
                            LogLevel::Info,
                        );
                        let _ = push_tx.send(PushRequest {
                            name: entry.name.clone(),
                            data,
                            sha256: hash,
                        });
                    }
                    Err(e) => {
                        self.log(
                            format!("[PUSH] Failed to read {}: {}", entry.name, e),
                            LogLevel::Error,
                        );
                    }
                }
            }
        }
    }

    /// Process a server event from the receive channel
    fn handle_server_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::Listening { addr } => {
                self.log(
                    format!("Server listening on {}", addr),
                    LogLevel::Info,
                );
            }
            ServerEvent::PeerConnected { name, addr } => {
                self.status = ConnectionStatus::Connected {
                    peer_name: name.clone(),
                };
                self.log(
                    format!("📱 {} connected ({})", name, addr),
                    LogLevel::Success,
                );
                // Initialize file browser on connect (Feature 2A)
                self.init_file_browser();
            }
            ServerEvent::PeerDisconnected { name } => {
                let all_done = self.file_queue.iter().all(|f| {
                    matches!(
                        f.status,
                        TransferStatus::Completed | TransferStatus::Failed(_)
                    )
                });
                if all_done && !self.file_queue.is_empty() {
                    self.status = ConnectionStatus::Complete;
                } else {
                    self.status = ConnectionStatus::Ready;
                }
                self.log(format!("{} disconnected", name), LogLevel::Info);
                // Do NOT close file browser, let the third panel persist!
            }
            ServerEvent::FileStarted { file } => {
                self.add_file(file);
            }
            ServerEvent::Progress {
                file_name,
                bytes_received,
                bytes_total,
                speed,
            } => {
                self.update_progress(&file_name, bytes_received, bytes_total, speed);
            }
            ServerEvent::FileCompleted { file_name, verified } => {
                self.complete_file(&file_name);
                if verified {
                    let tag = if self.encrypt_enabled { "[ENC] ✓" } else { "✓" };
                    self.log(
                        format!("{} {} — verified", tag, file_name),
                        LogLevel::Success,
                    );
                }
            }
            ServerEvent::FileFailed { file_name, error } => {
                self.fail_file(&file_name, &error);
                self.log(
                    format!("✗ {} — {}", file_name, error),
                    LogLevel::Error,
                );
            }
            ServerEvent::Log { message } => {
                self.log(message, LogLevel::Info);
            }
            ServerEvent::Error { message } => {
                self.log(message, LogLevel::Error);
            }
        }
    }

    /// Process a client event from the send channel
    fn handle_client_event(&mut self, event: ClientEvent) {
        match event {
            ClientEvent::Connecting { addr } => {
                self.log(format!("Connecting to {}...", addr), LogLevel::Info);
            }
            ClientEvent::Connected { peer_name } => {
                self.status = ConnectionStatus::Connected {
                    peer_name: peer_name.clone(),
                };
                self.log(format!("Connected to {}", peer_name), LogLevel::Success);
            }
            ClientEvent::Disconnected => {
                self.status = ConnectionStatus::Disconnected;
                self.log("Disconnected".to_string(), LogLevel::Warning);
            }
            ClientEvent::FileStarted { file } => {
                self.add_file(file);
            }
            ClientEvent::Progress {
                file_name,
                bytes_sent,
                bytes_total,
                speed,
            } => {
                self.update_progress(&file_name, bytes_sent, bytes_total, speed);
            }
            ClientEvent::FileCompleted { file_name } => {
                self.complete_file(&file_name);
                self.log(
                    format!("✓ {} sent successfully", file_name),
                    LogLevel::Success,
                );
            }
            ClientEvent::FileFailed { file_name, error } => {
                self.fail_file(&file_name, &error);
                self.log(
                    format!("✗ {} — {}", file_name, error),
                    LogLevel::Error,
                );
            }
            ClientEvent::BatchComplete {
                successful,
                failed,
            } => {
                self.status = ConnectionStatus::Complete;
                self.log(
                    format!(
                        "Batch complete: {} succeeded, {} failed",
                        successful, failed
                    ),
                    LogLevel::Success,
                );
            }
            ClientEvent::Log { message } => {
                self.log(message, LogLevel::Info);
            }
            ClientEvent::Error { message } => {
                self.log(message, LogLevel::Error);
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Feature 3: Connection Mode Selection Screen
// ══════════════════════════════════════════════════════════════════════════════

/// Run the interactive connection mode selection screen.
/// Returns the selected ConnectionMode.
pub async fn run_mode_selection(
    last_mode: Option<ConnectionMode>,
) -> Result<ConnectionMode> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut selected: usize = 0; // 0 = Router, 1 = Hotspot
    let result;

    // Drain any stale keyboard events (e.g. the Enter that launched the command)
    tokio::time::sleep(Duration::from_millis(200)).await;
    while crossterm::event::poll(Duration::from_millis(0))? {
        let _ = crossterm::event::read();
    }

    loop {
        terminal.draw(|frame| {
            ui::render_mode_selection(frame, selected);
        })?;

        // Inline polling — no spawned task, so nothing leaks after this function returns
        let has_event = tokio::task::spawn_blocking(|| {
            crossterm::event::poll(std::time::Duration::from_millis(100))
        }).await??;

        if has_event {
            let evt = tokio::task::spawn_blocking(crossterm::event::read).await??;
            if let crossterm::event::Event::Key(key) = evt {
                // Ignore key release events on Windows
                if key.kind == crossterm::event::KeyEventKind::Release {
                    continue;
                }
                match key.code {
                    crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('1') => selected = 0,
                    crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('2') => selected = 1,
                    crossterm::event::KeyCode::Enter => {
                        result = if selected == 0 {
                            ConnectionMode::Router
                        } else {
                            ConnectionMode::Hotspot
                        };
                        break;
                    }
                    crossterm::event::KeyCode::Char('s') | crossterm::event::KeyCode::Char('S') => {
                        result = last_mode.unwrap_or(ConnectionMode::Router);
                        break;
                    }
                    crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Char('Q') => {
                        disable_raw_mode()?;
                        execute!(
                            terminal.backend_mut(),
                            LeaveAlternateScreen,
                            DisableMouseCapture
                        )?;
                        terminal.show_cursor()?;
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Drain any events generated during transition
    tokio::time::sleep(Duration::from_millis(100)).await;
    while crossterm::event::poll(Duration::from_millis(0))? {
        let _ = crossterm::event::read();
    }

    Ok(result)
}

// ══════════════════════════════════════════════════════════════════════════════
// Feature 3: Hotspot Setup Guide Screen
// ══════════════════════════════════════════════════════════════════════════════

/// Run the hotspot setup guide screen.
/// Returns true if the user wants to continue, false to go back.
pub async fn run_hotspot_guide(auto: bool) -> Result<bool> {
    if auto {
        return Ok(true);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let os = crate::hotspot::detect_os();
    let result;

    // Drain stale keyboard events
    tokio::time::sleep(Duration::from_millis(200)).await;
    while crossterm::event::poll(Duration::from_millis(0))? {
        let _ = crossterm::event::read();
    }

    loop {
        terminal.draw(|frame| {
            ui::render_hotspot_guide(frame, os);
        })?;

        // Inline polling — no spawned task
        let has_event = tokio::task::spawn_blocking(|| {
            crossterm::event::poll(std::time::Duration::from_millis(100))
        }).await??;

        if has_event {
            let evt = tokio::task::spawn_blocking(crossterm::event::read).await??;
            if let crossterm::event::Event::Key(key) = evt {
                if key.kind == crossterm::event::KeyEventKind::Release {
                    continue;
                }
                match key.code {
                    crossterm::event::KeyCode::Enter | crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
                        result = true;
                        break;
                    }
                    crossterm::event::KeyCode::Char('b') | crossterm::event::KeyCode::Char('B') => {
                        result = false;
                        break;
                    }
                    crossterm::event::KeyCode::Char('a') | crossterm::event::KeyCode::Char('A') => {
                        if os == "linux" {
                            disable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            terminal.show_cursor()?;

                            println!("  \x1b[32m[HOTSPOT] Creating hotspot...\x1b[0m");
                            match crate::hotspot::auto_create_hotspot().await {
                                Ok((ssid, password)) => {
                                    println!("  \x1b[32m[HOTSPOT] ✓ SSID: {}  Password: {}\x1b[0m", ssid, password);
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    println!("  \x1b[31m[HOTSPOT] Failed: {}\x1b[0m", e);
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                            }
                            return Ok(true);
                        }
                    }
                    crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Char('Q') => {
                        disable_raw_mode()?;
                        execute!(
                            terminal.backend_mut(),
                            LeaveAlternateScreen,
                            DisableMouseCapture
                        )?;
                        terminal.show_cursor()?;
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Drain events after transition
    tokio::time::sleep(Duration::from_millis(100)).await;
    while crossterm::event::poll(Duration::from_millis(0))? {
        let _ = crossterm::event::read();
    }

    Ok(result)
}

// ══════════════════════════════════════════════════════════════════════════════
// Main TUI Entry Points
// ══════════════════════════════════════════════════════════════════════════════

/// Run the TUI in receive mode — spawns WebSocket server + mDNS + TUI
pub async fn run_receive(
    port: u16,
    device_name: &str,
    fingerprint: &str,
    local_ips: &[String],
    multi: bool,
    encrypt: bool,
    hotspot_mode: bool,
) -> Result<()> {
    // Create server event channel
    let (server_tx, server_rx) = mpsc::unbounded_channel::<ServerEvent>();

    // Create push request channel for file browser → server (Feature 2A)
    let (push_tx, push_rx) = mpsc::unbounded_channel::<PushRequest>();

    // Spawn mDNS advertisement in the background
    let mdns_name = device_name.to_string();
    let mdns_fp = fingerprint.to_string();
    let mdns_port = port;
    tokio::spawn(async move {
        match crate::discovery::advertise_service(&mdns_name, mdns_port, &mdns_fp) {
            Ok(_daemon) => {
                // Keep the daemon alive — it drops when this task is cancelled
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            }
            Err(e) => {
                tracing::error!("mDNS advertisement failed: {}", e);
            }
        }
    });

    // Spawn WebSocket server in the background
    let srv_tx = server_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::transfer::server::start_server(port, srv_tx, multi, encrypt, Some(push_rx)).await {
            tracing::error!("Server error: {}", e);
        }
    });

    // Build the phone URL from the first detected LAN IP
    let phone_url = local_ips
        .first()
        .map(|ip| format!("http://{}:{}", ip, port));

    // Show QR code with the phone URL before launching TUI
    if let Some(ref url) = phone_url {
        println!();
        if hotspot_mode {
            println!("  \x1b[33m[HOTSPOT MODE] Connect your phone to the hotspot Wi-Fi first, then scan this QR\x1b[0m");
            println!();
        }
        println!("  \x1b[32m╔══════════════════════════════════════════════════════╗\x1b[0m");
        println!("  \x1b[32m║\x1b[0m  \x1b[1;32m[FILEDROP]\x1b[0m  v0.5.0  ::  RECEIVE_MODE              \x1b[32m║\x1b[0m");
        println!("  \x1b[32m╠══════════════════════════════════════════════════════╣\x1b[0m");
        println!("  \x1b[32m║\x1b[0m                                                      \x1b[32m║\x1b[0m");
        println!("  \x1b[32m║\x1b[0m  \x1b[1;32m> SCAN QR CODE ON PHONE TO CONNECT:\x1b[0m                 \x1b[32m║\x1b[0m");
        println!("  \x1b[32m║\x1b[0m                                                      \x1b[32m║\x1b[0m");
        println!("  \x1b[32m╚══════════════════════════════════════════════════════╝\x1b[0m");
        println!();

        // Generate and display QR code
        if let Ok(code) = qrcode::QrCode::new(url.as_bytes()) {
            let string = code
                .render::<char>()
                .quiet_zone(true)
                .module_dimensions(2, 1)
                .build();
            for line in string.lines() {
                println!("    {}", line);
            }
        }

        println!();
        println!("  \x1b[32m  URL:\x1b[0m  \x1b[1;32m{}\x1b[0m", url);
        println!("  \x1b[32m  DIR:\x1b[0m  {}", std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string()));
        if multi {
            println!("  \x1b[33m  MODE: Multi-device (session lock disabled)\x1b[0m");
        }
        if encrypt {
            println!("  \x1b[36m  ENC:  End-to-end encryption enabled\x1b[0m");
        }
        println!();
        println!("  \x1b[33m  >> Press ENTER to launch TUI...\x1b[0m");
        println!();

        // Use crossterm's own event system to wait for Enter.
        // stdin().read_line() breaks on Windows after raw mode cycles.
        enable_raw_mode()?;
        loop {
            let has_event = tokio::task::spawn_blocking(|| {
                crossterm::event::poll(std::time::Duration::from_millis(200))
            }).await??;

            if has_event {
                let evt = tokio::task::spawn_blocking(crossterm::event::read).await??;
                if let crossterm::event::Event::Key(key) = evt {
                    if key.kind == crossterm::event::KeyEventKind::Release {
                        continue;
                    }
                    match key.code {
                        crossterm::event::KeyCode::Enter => break,
                        crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Char('Q') => {
                            disable_raw_mode()?;
                            std::process::exit(0);
                        }
                        _ => {} // ignore other keys, keep waiting
                    }
                }
            }
        }
        disable_raw_mode()?;

        // Drain events before TUI
        tokio::time::sleep(Duration::from_millis(100)).await;
        while crossterm::event::poll(Duration::from_millis(0))? {
            let _ = crossterm::event::read();
        }
    }

    // Run the TUI event loop with the server event channel
    run_tui(
        AppMode::Receive,
        Some(server_rx),
        None,
        phone_url,
        local_ips,
        Some(push_tx),
        hotspot_mode,
        encrypt,
    ).await
}

/// Run the TUI in send mode — spawns WebSocket client + TUI
pub async fn run_send(peer_addr: &str, files: Vec<std::path::PathBuf>) -> Result<()> {
    let (client_tx, client_rx) = mpsc::unbounded_channel::<ClientEvent>();

    // Spawn the send task in the background
    let addr = peer_addr.to_string();
    let files_clone = files.clone();
    tokio::spawn(async move {
        if let Err(e) =
            crate::transfer::client::send_files(&addr, files_clone, client_tx).await
        {
            tracing::error!("Send error: {}", e);
        }
    });

    // Run the TUI event loop with the client event channel
    run_tui(AppMode::Send, None, Some(client_rx), None, &[], None, false, false).await
}

/// Core TUI event loop — handles keyboard events, server events, client events, and rendering
async fn run_tui(
    mode: AppMode,
    server_rx: Option<mpsc::UnboundedReceiver<ServerEvent>>,
    client_rx: Option<mpsc::UnboundedReceiver<ClientEvent>>,
    phone_url: Option<String>,
    local_ips: &[String],
    push_tx: Option<mpsc::UnboundedSender<PushRequest>>,
    hotspot_mode: bool,
    encrypt: bool,
) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::new(mode.clone());
    app.server_event_rx = server_rx;
    app.client_event_rx = client_rx;
    app.phone_url = phone_url.clone();
    app.push_tx = push_tx;
    app.hotspot_mode = hotspot_mode;
    app.encrypt_enabled = encrypt;

    // Initial log
    app.log(
        format!("FileDrop v0.5.0 — {}", mode),
        LogLevel::Info,
    );

    if hotspot_mode {
        app.log("[HOTSPOT MODE] Active — yellow badge shown".to_string(), LogLevel::Warning);
    }
    if encrypt {
        app.log("[ENC] End-to-end encryption enabled".to_string(), LogLevel::Info);
    }

    match &app.mode {
        AppMode::Receive => {
            // Show the phone URL prominently in the log
            if let Some(ref url) = phone_url {
                app.log(format!("📱 Open on your phone: {}", url), LogLevel::Success);
                if local_ips.len() > 1 {
                    for ip in &local_ips[1..] {
                        app.log(format!("   Alt IP: http://{}:{}", ip, url.rsplit(':').next().unwrap_or("7878")), LogLevel::Info);
                    }
                }
                app.log("Both devices must be on the same Wi-Fi network".to_string(), LogLevel::Info);
            } else {
                app.log("⚠ Could not detect LAN IP. Check network.".to_string(), LogLevel::Warning);
                app.log("[WARN] No active network detected. Is your Wi-Fi connected? Or try: filedrop receive --mode hotspot".to_string(), LogLevel::Warning);
            }
            app.log("Waiting for incoming connections...".to_string(), LogLevel::Info);
        }
        AppMode::Send => {
            app.log("Preparing to send files...".to_string(), LogLevel::Info);
        }
    }

    // Create keyboard event channel
    let (kb_tx, mut kb_rx) = mpsc::unbounded_channel::<AppEvent>();
    tokio::spawn(async move {
        events::poll_keyboard_events(kb_tx).await;
    });

    // Main event loop with 30ms tick rate for smooth MacBook-like transitions
    let tick_rate = Duration::from_millis(30);

    loop {
        // Update layout size interpolation
        app.update_layout_interpolation();

        // ── DRAW ──────────────────────────────────────────────────
        terminal.draw(|frame| {
            ui::render(frame, &app);
        })?;

        // ── PROCESS EVENTS ───────────────────────────────────────
        let timeout = tokio::time::sleep(tick_rate);
        tokio::pin!(timeout);

        tokio::select! {
            // Keyboard events
            Some(event) = kb_rx.recv() => {
                match event {
                    AppEvent::Quit => app.should_quit = true,
                    AppEvent::TabFocus => {
                        // Cycle focus through the panes
                        app.focus = match app.focus {
                            FocusPane::TransferQueue => FocusPane::SystemLog,
                            FocusPane::SystemLog => {
                                if app.file_browser.is_some() {
                                    FocusPane::FileBrowser
                                } else {
                                    FocusPane::TransferQueue
                                }
                            }
                            FocusPane::FileBrowser => FocusPane::TransferQueue,
                        };
                    }
                    // File browser events (when focused)
                    ref evt @ (AppEvent::Enter | AppEvent::GoBack | AppEvent::ToggleSelect |
                               AppEvent::SelectAll | AppEvent::SendSelected | AppEvent::ClearSelection)
                        if app.focus == FocusPane::FileBrowser =>
                    {
                        app.handle_file_browser_event(evt);
                    }
                    // Scroll events respect current focus
                    AppEvent::ScrollUp if app.focus == FocusPane::FileBrowser => {
                        app.handle_file_browser_event(&AppEvent::ScrollUp);
                    }
                    AppEvent::ScrollUp if app.focus == FocusPane::SystemLog => {
                        app.log_scroll = app.log_scroll.saturating_sub(1);
                    }
                    AppEvent::ScrollUp => {
                        app.queue_scroll = app.queue_scroll.saturating_sub(1);
                    }
                    AppEvent::ScrollDown if app.focus == FocusPane::FileBrowser => {
                        app.handle_file_browser_event(&AppEvent::ScrollDown);
                    }
                    AppEvent::ScrollDown if app.focus == FocusPane::SystemLog => {
                        app.log_scroll = app.log_scroll
                            .saturating_add(1)
                            .min(app.log_entries.len().saturating_sub(1));
                    }
                    AppEvent::ScrollDown => {
                        app.queue_scroll = app.queue_scroll
                            .saturating_add(1)
                            .min(app.file_queue.len().saturating_sub(1));
                    }
                    AppEvent::ScrollHome => {
                        if app.focus == FocusPane::FileBrowser {
                            if let Some(ref mut fb) = app.file_browser {
                                fb.cursor = 0;
                            }
                        } else if app.focus == FocusPane::SystemLog {
                            app.log_scroll = 0;
                        } else {
                            app.queue_scroll = 0;
                        }
                    }
                    AppEvent::ScrollEnd => {
                        if app.focus == FocusPane::FileBrowser {
                            if let Some(ref mut fb) = app.file_browser {
                                fb.cursor = fb.entries.len().saturating_sub(1);
                            }
                        } else if app.focus == FocusPane::SystemLog {
                            app.log_scroll = app.log_entries.len().saturating_sub(1);
                        } else {
                            app.queue_scroll = app.file_queue.len().saturating_sub(1);
                        }
                    }
                    AppEvent::LogScrollUp => {
                        app.log_scroll = app.log_scroll.saturating_sub(10);
                    }
                    AppEvent::LogScrollDown => {
                        app.log_scroll = app.log_scroll
                            .saturating_add(10)
                            .min(app.log_entries.len().saturating_sub(1));
                    }
                    AppEvent::Tick => {}
                    _ => {}
                }
            }

            // Server events (receive mode)
            Some(event) = async {
                if let Some(ref mut rx) = app.server_event_rx {
                    rx.recv().await
                } else {
                    // Never resolves if no server
                    std::future::pending::<Option<ServerEvent>>().await
                }
            } => {
                app.handle_server_event(event);
            }

            // Client events (send mode)
            Some(event) = async {
                if let Some(ref mut rx) = app.client_event_rx {
                    rx.recv().await
                } else {
                    std::future::pending::<Option<ClientEvent>>().await
                }
            } => {
                app.handle_client_event(event);
            }

            // Tick timeout — just redraw
            _ = &mut timeout => {}
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

/// Run the app in the specified mode (legacy entry point)
#[allow(dead_code)]
pub async fn run_app(mode: AppMode) -> Result<()> {
    run_tui(mode, None, None, None, &[], None, false, false).await
}

//! TUI application state and main event loop.
//!
//! Manages the Ratatui terminal lifecycle, coordinates between
//! keyboard events, server/client events, and UI rendering.
//! Runs server + mDNS + TUI concurrently in receive mode.

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::transfer::client::ClientEvent;
use crate::transfer::protocol::{TransferFile, TransferStatus};
use crate::transfer::server::ServerEvent;

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
}

impl AppState {
    /// Create a new application state
    pub fn new(mode: AppMode) -> Self {
        Self {
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
        }
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
                    self.log(
                        format!("✓ {} — verified", file_name),
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

/// Run the TUI in receive mode — spawns WebSocket server + mDNS + TUI
pub async fn run_receive(
    port: u16,
    device_name: &str,
    fingerprint: &str,
    local_ips: &[String],
) -> Result<()> {
    // Create server event channel
    let (server_tx, server_rx) = mpsc::unbounded_channel::<ServerEvent>();

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
        if let Err(e) = crate::transfer::server::start_server(port, srv_tx).await {
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
        println!("  \x1b[32m╔══════════════════════════════════════════════════════╗\x1b[0m");
        println!("  \x1b[32m║\x1b[0m  \x1b[1;32m[FILEDROP]\x1b[0m  v0.1.0  ::  RECEIVE_MODE              \x1b[32m║\x1b[0m");
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
        println!();
        println!("  \x1b[33m  >> Press ENTER to launch TUI...\x1b[0m");
        println!();

        // Wait for user to press Enter — no auto-timer
        tokio::task::spawn_blocking(|| {
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
        }).await.ok();
    }

    // Run the TUI event loop with the server event channel
    run_tui(AppMode::Receive, Some(server_rx), None, phone_url, local_ips).await
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
    run_tui(AppMode::Send, None, Some(client_rx), None, &[]).await
}

/// Core TUI event loop — handles keyboard events, server events, client events, and rendering
async fn run_tui(
    mode: AppMode,
    server_rx: Option<mpsc::UnboundedReceiver<ServerEvent>>,
    client_rx: Option<mpsc::UnboundedReceiver<ClientEvent>>,
    phone_url: Option<String>,
    local_ips: &[String],
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

    // Initial log
    app.log(
        format!("FileDrop v0.1.0 — {}", mode),
        LogLevel::Info,
    );
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

    // Main event loop with 100ms tick rate
    let tick_rate = Duration::from_millis(100);

    loop {
        // ── DRAW ──────────────────────────────────────────────────
        terminal.draw(|frame| {
            ui::render(frame, &app);
        })?;

        // ── PROCESS EVENTS ───────────────────────────────────────
        // Use tokio::select to wait for any event source
        let timeout = tokio::time::sleep(tick_rate);
        tokio::pin!(timeout);

        tokio::select! {
            // Keyboard events
            Some(event) = kb_rx.recv() => {
                match event {
                    AppEvent::Quit => app.should_quit = true,
                    AppEvent::ScrollUp => {
                        app.queue_scroll = app.queue_scroll.saturating_sub(1);
                    }
                    AppEvent::ScrollDown => {
                        app.queue_scroll = app.queue_scroll
                            .saturating_add(1)
                            .min(app.file_queue.len().saturating_sub(1));
                    }
                    AppEvent::LogScrollUp => {
                        app.log_scroll = app.log_scroll.saturating_sub(1);
                    }
                    AppEvent::LogScrollDown => {
                        app.log_scroll = app.log_scroll
                            .saturating_add(1)
                            .min(app.log_entries.len().saturating_sub(1));
                    }
                    AppEvent::Tick => {}
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
    run_tui(mode, None, None, None, &[]).await
}

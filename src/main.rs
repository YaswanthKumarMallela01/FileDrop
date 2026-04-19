//! FileDrop — Secure, fast, local Wi-Fi file transfer
//!
//! A CLI tool for transferring files between a laptop and a paired phone
//! over local Wi-Fi using mTLS and WebSocket protocol.

mod config;
mod discovery;
mod security;
mod transfer;
mod tui;
mod web;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// FileDrop — Secure local Wi-Fi file transfer
#[derive(Parser, Debug)]
#[command(
    name = "filedrop",
    version = "0.1.0",
    about = "Secure, fast, local Wi-Fi file transfer between laptop and phone",
    long_about = "FileDrop enables secure file transfer between your laptop and paired phone \
                  over local Wi-Fi. No internet, no cloud — just fast, encrypted transfers \
                  using mutual TLS with certificate pinning."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate QR code for one-time phone pairing
    Pair,

    /// Start TUI server to receive files into the current directory
    Receive,

    /// Send a file or directory to a paired phone
    Send {
        /// Path to file or directory to send (use '.' for all files in CWD)
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Address of peer to send to (ip:port). If omitted, uses mDNS discovery.
        #[arg(short, long)]
        addr: Option<String>,
    },

    /// List all paired devices
    Peers,

    /// Remove a paired device
    Unpair {
        /// Name of the peer to remove
        #[arg(value_name = "NAME")]
        name: String,
    },

    /// Launch a demo TUI with simulated transfers (for testing)
    Demo,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing (only for non-TUI commands; TUI suppresses stdout)
    let cli = Cli::parse();

    // Ensure config directories exist
    config::ensure_directories()?;

    match cli.command {
        Commands::Pair => {
            init_tracing();
            println!();
            println!("  ╔══════════════════════════════════════╗");
            println!("  ║       FileDrop v0.1.0 — Pairing      ║");
            println!("  ╚══════════════════════════════════════╝");
            println!();
            security::pairing::start_pairing().await?;
        }

        Commands::Receive => {
            // Load config and ensure certs
            let cfg = config::load_config()?;
            let bundle = security::certs::ensure_certificates()?;

            // Detect all local network IPs so the TUI can show them
            let local_ips = get_local_ips();

            // Launch receive mode: TUI + server + mDNS
            // The phone URL is shown persistently inside the TUI header,
            // not printed to stdout (which gets wiped by alternate screen).
            tui::app::run_receive(
                cfg.port,
                &cfg.device_name,
                &bundle.fingerprint,
                &local_ips,
            )
            .await?;
        }

        Commands::Send { path, addr } => {
            // Load config and ensure certs
            let _cfg = config::load_config()?;
            let _bundle = security::certs::ensure_certificates()?;

            // Validate path
            if !path.exists() {
                anyhow::bail!("Path does not exist: {}", path.display());
            }

            // Collect files to send
            let files = transfer::chunker::collect_files(&path).await?;
            if files.is_empty() {
                anyhow::bail!("No files found at: {}", path.display());
            }

            // Determine peer address
            let peer_addr = if let Some(a) = addr {
                a
            } else {
                // Auto-discover via mDNS
                println!("  🔍 Discovering FileDrop peers on the network...");
                transfer::client::discover_peer(10).await?
            };

            // Launch send mode: TUI + client
            tui::app::run_send(&peer_addr, files).await?;
        }

        Commands::Peers => {
            init_tracing();
            list_peers()?;
        }

        Commands::Unpair { name } => {
            init_tracing();
            unpair_device(&name)?;
        }

        Commands::Demo => {
            run_demo_mode().await?;
        }
    }

    Ok(())
}

/// Initialize tracing for non-TUI commands
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("filedrop=info".parse().unwrap()),
        )
        .with_target(false)
        .init();
}

/// List all paired devices
fn list_peers() -> anyhow::Result<()> {
    let peers = security::certs::list_peers()?;

    if peers.is_empty() {
        println!();
        println!("  No paired devices found.");
        println!("  Run 'filedrop pair' to pair a new device.");
        println!();
        return Ok(());
    }

    println!();
    println!("  ╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("  ║  Paired Devices                                                             ║");
    println!("  ╠══════════════════════╦════════════════════════════════════════╦══════════════╣");
    println!(
        "  ║ {:<20} ║ {:<38} ║ {:<12} ║",
        "Name", "Fingerprint", "Paired On"
    );
    println!("  ╠══════════════════════╬════════════════════════════════════════╬══════════════╣");

    for peer in &peers {
        let fp_short = if peer.fingerprint.len() > 38 {
            format!("{}…", &peer.fingerprint[..37])
        } else {
            peer.fingerprint.clone()
        };
        let date_short = if peer.paired_at.len() > 12 {
            peer.paired_at[..12].to_string()
        } else {
            peer.paired_at.clone()
        };
        println!(
            "  ║ {:<20} ║ {:<38} ║ {:<12} ║",
            peer.name, fp_short, date_short
        );
    }

    println!("  ╚══════════════════════╩════════════════════════════════════════╩══════════════╝");
    println!();
    println!("  To remove a device: filedrop unpair <name>");
    println!();

    Ok(())
}

/// Remove a paired device by name
fn unpair_device(name: &str) -> anyhow::Result<()> {
    security::certs::remove_peer(name)?;
    println!();
    println!("  ✓ Successfully unpaired device: {}", name);
    println!();
    Ok(())
}

/// Get all local LAN IP addresses (filters out loopback and link-local)
fn get_local_ips() -> Vec<String> {
    let mut ips = Vec::new();

    // Method 1: UDP socket trick (most reliable for "primary" IP)
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip().to_string();
                if !ips.contains(&ip) {
                    ips.push(ip);
                }
            }
        }
    }

    // Method 2: Enumerate all network interfaces via local_ip_address crate
    // (fallback — scan common private ranges by binding)
    for base in ["192.168.", "10.", "172."] {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
            // Try connecting to a dummy address in each range
            let target = format!("{}1.1:80", base);
            if socket.connect(&target).is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    let ip = addr.ip().to_string();
                    if !ips.contains(&ip) {
                        ips.push(ip);
                    }
                }
            }
        }
    }

    // If nothing found, at least return localhost
    if ips.is_empty() {
        ips.push("127.0.0.1".to_string());
    }

    ips
}

/// Run demo mode — simulates a file transfer for visual TUI testing
async fn run_demo_mode() -> anyhow::Result<()> {
    use crate::transfer::protocol::{TransferFile, TransferStatus};
    use crate::tui::app::{AppMode, AppState, LogLevel};
    use crossterm::{
        event::{DisableMouseCapture, EnableMouseCapture},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::io;
    use std::time::Duration;
    use tokio::sync::mpsc;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = AppState::new(AppMode::Receive);

    // Simulate initial state
    app.log("FileDrop v0.1.0 — RECEIVE MODE".into(), LogLevel::Info);
    app.log("Server listening on ws://0.0.0.0:7878/ws".into(), LogLevel::Info);
    app.log("mDNS: Advertising 'my-laptop' on _filedrop._tcp.local".into(), LogLevel::Info);
    app.log("Waiting for incoming connections...".into(), LogLevel::Info);

    // Create keyboard event channel
    let (kb_tx, mut kb_rx) = mpsc::unbounded_channel::<tui::events::AppEvent>();
    tokio::spawn(async move {
        tui::events::poll_keyboard_events(kb_tx).await;
    });

    // Simulation phases
    let mut tick = 0u64;
    let tick_rate = Duration::from_millis(80);

    // Define simulated files
    let sim_files = vec![
        ("photo_001.jpg", 2_400_000u64),
        ("vacation_video.mp4", 845_000_000u64),
        ("document.pdf", 420_000u64),
        ("presentation.pptx", 8_300_000u64),
        ("backup.zip", 156_000_000u64),
    ];

    loop {
        // Draw
        terminal.draw(|frame| {
            tui::ui::render(frame, &app);
        })?;

        // Process keyboard events
        let timeout = tokio::time::sleep(tick_rate);
        tokio::pin!(timeout);

        tokio::select! {
            Some(event) = kb_rx.recv() => {
                match event {
                    tui::events::AppEvent::Quit => break,
                    tui::events::AppEvent::ScrollUp => {
                        app.queue_scroll = app.queue_scroll.saturating_sub(1);
                    }
                    tui::events::AppEvent::ScrollDown => {
                        app.queue_scroll = app.queue_scroll
                            .saturating_add(1)
                            .min(app.file_queue.len().saturating_sub(1));
                    }
                    _ => {}
                }
            }
            _ = &mut timeout => {}
        }

        // ── Simulation Logic ──────────────────────────────────────
        tick += 1;

        // Phase 1: Connect at tick 15
        if tick == 15 {
            app.status = tui::app::ConnectionStatus::Connected {
                peer_name: "iPhone 15 Pro".into(),
            };
            app.log("📱 iPhone 15 Pro connected (192.168.1.42)".into(), LogLevel::Success);
        }

        // Phase 2: Add files to queue at tick 25
        if tick == 25 {
            for (name, size) in &sim_files {
                let tf = TransferFile::new(name.to_string(), *size, "abcdef1234567890".into());
                app.add_file(tf);
            }
            app.log(format!("Batch: {} files incoming", sim_files.len()), LogLevel::Info);
        }

        // Phase 3: Simulate transfers one by one
        if tick > 30 {
            // Find first non-completed file
            let active_idx = app.file_queue.iter().position(|f| {
                !matches!(f.status, TransferStatus::Completed)
            });

            if let Some(idx) = active_idx {
                let file = &app.file_queue[idx];
                let file_name = file.name.clone();
                let file_size = file.size;
                let current_bytes = file.bytes_transferred;

                // Simulate ~50MB/s transfer with some jitter
                let chunk = (50_000_000.0 * (tick_rate.as_secs_f64())
                    * (0.7 + 0.6 * ((tick as f64 * 0.3).sin().abs()))) as u64;
                let new_bytes = (current_bytes + chunk).min(file_size);
                let speed = chunk as f64 / tick_rate.as_secs_f64();

                if new_bytes >= file_size {
                    // File completed
                    app.complete_file(&file_name);
                    app.log(
                        format!("✓ '{}' saved — verified", file_name),
                        LogLevel::Success,
                    );
                } else {
                    app.update_progress(&file_name, new_bytes, file_size, speed);

                    // Log the start of a new file
                    if current_bytes == 0 {
                        app.log(
                            format!(
                                "Receiving '{}' ({})...",
                                file_name,
                                transfer::protocol::format_bytes(file_size)
                            ),
                            LogLevel::Info,
                        );
                    }
                }
            } else if !app.file_queue.is_empty() {
                // All files done
                if app.status != tui::app::ConnectionStatus::Complete {
                    app.status = tui::app::ConnectionStatus::Complete;
                    app.log("All transfers complete!".into(), LogLevel::Success);
                    app.log("Press Q to quit".into(), LogLevel::Info);
                }
            }
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

//! FileDrop — Secure, fast, local Wi-Fi file transfer
//!
//! A CLI tool for transferring files between a laptop and a paired phone
//! over local Wi-Fi using mTLS and WebSocket protocol.

mod config;
mod discovery;
mod hotspot;
mod security;
mod share;
mod sync;
mod transfer;
mod tui;
mod web;
mod installer;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// FileDrop — Secure local Wi-Fi file transfer
#[derive(Parser, Debug)]
#[command(
    name = "filedrop",
    version = "0.3.1",
    about = "Secure, fast, local Wi-Fi file transfer between laptop and phone",
    long_about = "FileDrop enables secure file transfer between your laptop and paired phone \
                  over local Wi-Fi. No internet, no cloud — just fast, encrypted transfers \
                  using mutual TLS with certificate pinning."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate QR code for one-time phone pairing
    Pair,

    /// Start TUI server to receive files into the current directory
    Receive {
        /// Connection mode: skip the interactive prompt
        #[arg(long, value_parser = ["router", "hotspot"])]
        mode: Option<String>,

        /// Allow multiple simultaneous connections (disables single-device lock)
        #[arg(long)]
        multi: bool,

        /// Enable end-to-end encryption (AES-256-GCM via ECDH key exchange)
        #[arg(long)]
        encrypt: bool,
    },

    /// Send a file or directory to a paired phone
    Send {
        /// Path to file or directory to send (use '.' for all files in CWD)
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Address of peer to send to (ip:port). If omitted, uses mDNS discovery.
        #[arg(short, long)]
        addr: Option<String>,
    },

    /// Create an ephemeral one-time share link for a file or folder
    Share {
        /// Path to file or folder to share (defaults to current directory if omitted)
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,

        /// Link expiration duration (e.g. "10m", "1h", "30s")
        #[arg(long)]
        expires: Option<String>,

        /// Link expires after the first download
        #[arg(long)]
        once: bool,

        /// Optional 4-digit PIN protection
        #[arg(long)]
        pin: Option<String>,

        /// Connection mode (router or hotspot)
        #[arg(long)]
        mode: Option<String>,
    },

    /// Watch and sync a folder to a paired device over LAN
    Sync {
        /// Path to the local folder to sync
        #[arg(value_name = "FOLDER")]
        folder: Option<PathBuf>,

        /// IP address of the peer to sync with (source mode)
        #[arg(long, value_name = "IP")]
        with: Option<String>,

        /// Run as listener (receive mode) instead of source
        #[arg(long)]
        listen: bool,

        /// Directory to save received files (listener mode)
        #[arg(long, value_name = "PATH")]
        save: Option<PathBuf>,
    },

    /// Guide to create a direct device connection (no router needed)
    Hotspot {
        /// Auto-create hotspot via nmcli (Linux only)
        #[arg(long)]
        auto: bool,
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

    let command = if let Some(cmd) = cli.command {
        cmd
    } else {
        // No command provided (e.g. double-clicked the executable)
        // Show the interactive main menu
        match tui::main_menu::run_main_menu().await? {
            tui::main_menu::MenuAction::RunCommand(cmd) => cmd,
            tui::main_menu::MenuAction::Install => {
                installer::install_self()?;
                return Ok(());
            }
            tui::main_menu::MenuAction::Exit => return Ok(()),
        }
    };

    match command {
        Commands::Pair => {
            init_tracing();
            println!();
            println!("  ╔══════════════════════════════════════╗");
            println!("  ║       FileDrop v0.3.1 — Pairing      ║");
            println!("  ╚══════════════════════════════════════╝");
            println!();
            security::pairing::start_pairing().await?;
        }

        Commands::Receive { mode, multi, encrypt } => {
            // Load config and ensure certs
            let mut cfg = config::load_config()?;
            let bundle = security::certs::ensure_certificates()?;

            // Determine connection mode
            let connection_mode = if let Some(mode_str) = mode {
                // --mode flag: skip prompt entirely
                hotspot::ConnectionMode::from_str(&mode_str)
                    .unwrap_or(hotspot::ConnectionMode::Router)
            } else {
                // Show interactive connection mode selection
                let last_mode = cfg.last_connection_mode.as_deref()
                    .and_then(hotspot::ConnectionMode::from_str);
                let selected = tui::app::run_mode_selection(last_mode).await?;

                // Persist the choice
                cfg.last_connection_mode = Some(selected.as_str().to_string());
                let _ = config::save_config(&cfg);

                selected
            };

            // If hotspot mode, show the setup guide first
            if connection_mode == hotspot::ConnectionMode::Hotspot {
                let should_continue = tui::app::run_hotspot_guide(false).await?;
                if !should_continue {
                    // User pressed B to go back — restart
                    return Ok(());
                }
            }

            // Detect all local network IPs
            let local_ips = get_local_ips(connection_mode == hotspot::ConnectionMode::Hotspot);

            // Launch receive mode: TUI + server + mDNS
            tui::app::run_receive(
                cfg.port,
                &cfg.device_name,
                &bundle.fingerprint,
                &local_ips,
                multi,
                encrypt,
                connection_mode == hotspot::ConnectionMode::Hotspot,
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

        Commands::Share { path, expires, once, pin, mode } => {
            let mut cfg = config::load_config()?;
            let _bundle = security::certs::ensure_certificates()?;

            // Determine if we should bypass the TUI configurator
            let use_tui = expires.is_none() && !once && pin.is_none();

            // 1. Determine connection mode
            let connection_mode = if let Some(ref mode_str) = mode {
                hotspot::ConnectionMode::from_str(mode_str)
                    .unwrap_or(hotspot::ConnectionMode::Router)
            } else {
                let last_mode = cfg.last_connection_mode.as_deref()
                    .and_then(hotspot::ConnectionMode::from_str);
                
                if use_tui {
                    let selected = tui::app::run_mode_selection(last_mode).await?;
                    // Persist choice
                    cfg.last_connection_mode = Some(selected.as_str().to_string());
                    let _ = config::save_config(&cfg);
                    selected
                } else {
                    last_mode.unwrap_or(hotspot::ConnectionMode::Router)
                }
            };

            // 2. If Hotspot mode and using TUI, show the setup guide
            if connection_mode == hotspot::ConnectionMode::Hotspot && use_tui {
                let should_continue = tui::app::run_hotspot_guide(false).await?;
                if !should_continue {
                    return Ok(());
                }
            }

            // 3. Resolve starting path and configuration
            let start_path = path.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

            let share_config = if use_tui {
                let sc = tui::share_configurator::run_share_configurator(start_path, connection_mode.clone()).await?;
                match sc {
                    Some(cfg) => cfg,
                    None => {
                        println!("  \x1b[33m[SHARE] Sharing cancelled by user.\x1b[0m");
                        return Ok(());
                    }
                }
            } else {
                // Direct CLI mode: parse inputs directly
                let duration_str = expires.as_deref().unwrap_or("15m");
                let duration = share::server::parse_duration(duration_str)?;
                tui::share_configurator::ShareConfig {
                    selected_paths: vec![start_path],
                    expires: duration,
                    once,
                    pin,
                }
            };

            // 4. Detect local IPs
            let local_ips = get_local_ips(connection_mode == hotspot::ConnectionMode::Hotspot);

            // 5. Run the share server with selected files and config
            share::server::run_share(
                share_config.selected_paths,
                share_config.expires,
                share_config.once,
                share_config.pin,
                &local_ips,
                connection_mode == hotspot::ConnectionMode::Hotspot,
                &cfg.device_name,
            ).await?;
        }

        Commands::Sync { folder, with, listen, save } => {
            let (tui_tx, _tui_rx) = tokio::sync::mpsc::unbounded_channel();

            if listen {
                // Listener mode
                let save_dir = save.unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                });
                sync::server::run_listener(save_dir, tui_tx).await?;
            } else {
                // Source mode
                let sync_folder = folder.unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                });
                let peer_ip = with.ok_or_else(|| {
                    anyhow::anyhow!("Source mode requires --with <peer_ip>")
                })?;

                if !sync_folder.exists() {
                    anyhow::bail!("Folder does not exist: {}", sync_folder.display());
                }

                sync::server::run_source(sync_folder, peer_ip, tui_tx).await?;
            }
        }

        Commands::Hotspot { auto } => {
            if auto {
                // Linux auto-setup
                match hotspot::auto_create_hotspot().await {
                    Ok((ssid, password)) => {
                        println!();
                        println!("  \x1b[32m[HOTSPOT] ✓ SSID: {}  Password: {}\x1b[0m", ssid, password);
                        println!();
                        // Continue to receive mode
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                    Err(e) => {
                        println!("  \x1b[31m[HOTSPOT] Auto-setup failed: {}\x1b[0m", e);
                        println!("  \x1b[33m  Auto-setup is only available on Linux.\x1b[0m");
                        return Ok(());
                    }
                }
            }

            // Show hotspot guide, then start receiver
            let should_continue = tui::app::run_hotspot_guide(auto).await?;
            if should_continue {
                let cfg = config::load_config()?;
                let bundle = security::certs::ensure_certificates()?;
                let local_ips = get_local_ips(true);

                tui::app::run_receive(
                    cfg.port,
                    &cfg.device_name,
                    &bundle.fingerprint,
                    &local_ips,
                    false,
                    false,
                    true, // hotspot mode
                )
                .await?;
            }
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
fn get_local_ips(hotspot_mode: bool) -> Vec<String> {
    let mut ips = Vec::new();

    // 1. On Windows, use Get-NetIPAddress with active physical adapter filtering.
    // This is extremely precise and avoids choosing WSL/Hyper-V/VirtualBox network cards as primary.
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let command_str = "Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $desc = (Get-NetAdapter -InterfaceIndex $_.InterfaceIndex -ErrorAction SilentlyContinue).InterfaceDescription; ( $null -ne $desc -and $desc -notlike '*VirtualBox*' -and $desc -notlike '*Hyper-V*' -and $desc -notlike '*VMware*' ) -and $_.InterfaceAlias -notlike '*Loopback*' -and $_.InterfaceAlias -notlike '*Bluetooth*' } | Select-Object -ExpandProperty IPAddress";
        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", command_str])
            .output();
        if let Ok(out) = output {
            let stdout_str = String::from_utf8_lossy(&out.stdout);
            for line in stdout_str.lines() {
                let ip = line.trim().to_string();
                if !ip.is_empty() && ip != "127.0.0.1" && !ip.starts_with("169.254.") {
                    if !ips.contains(&ip) {
                        ips.push(ip);
                    }
                }
            }
        }
    }

    // 2. UDP socket trick (cross-platform fallback or for non-Windows platforms)
    if ips.is_empty() {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if socket.connect("8.8.8.8:80").is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    let ip = addr.ip().to_string();
                    if ip != "127.0.0.1" && !ips.contains(&ip) {
                        ips.push(ip);
                    }
                }
            }
        }
    }

    // 3. Try dummy connections to common local gateway ranges as final fallback
    if ips.is_empty() {
        for base in ["192.168.", "10.", "172."] {
            for host in ["1.1", "137.1", "56.1"] {
                if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
                    let target = format!("{}{}:80", base, host);
                    if socket.connect(&target).is_ok() {
                        if let Ok(addr) = socket.local_addr() {
                            let ip = addr.ip().to_string();
                            if ip != "127.0.0.1" && !ips.contains(&ip) {
                                ips.push(ip);
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Double check and filter out any virtual subnets in case Windows fallback ran without Get-NetAdapter correlation
    let mut clean_ips = Vec::new();
    let mut virtual_ips = Vec::new();

    for ip in ips {
        let is_virtual = ip.starts_with("192.168.56.") || // VirtualBox default
                         (ip.starts_with("172.") && {
                             // Check if it's in the 172.16.x.x - 172.31.x.x private Class B block (typical for WSL2 and Hyper-V)
                             if let Some(second_octet) = ip.split('.').nth(1).and_then(|s| s.parse::<u8>().ok()) {
                                 (16..=31).contains(&second_octet)
                             } else {
                                 false
                             }
                         });

        if is_virtual {
            virtual_ips.push(ip);
        } else {
            clean_ips.push(ip);
        }
    }
    clean_ips.extend(virtual_ips);
    let mut ips = clean_ips;

    // 5. If in hotspot mode, prioritize the hotspot IP range!
    // On Windows, the default Mobile Hotspot IP is 192.168.137.1.
    // If we find any IP starting with 192.168.137. in our list, move it to the front!
    if hotspot_mode {
        let hotspot_index = ips.iter().position(|ip| ip.starts_with("192.168.137."));
        if let Some(idx) = hotspot_index {
            let hotspot_ip = ips.remove(idx);
            ips.insert(0, hotspot_ip);
        } else {
            // If the adapter is active but not captured in fallback, explicitly add it on Windows
            #[cfg(target_os = "windows")]
            {
                ips.insert(0, "192.168.137.1".to_string());
            }
        }
    }

    // Ensure we have at least loopback if list is empty
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
    app.log("FileDrop v0.3.1 — RECEIVE MODE".into(), LogLevel::Info);
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

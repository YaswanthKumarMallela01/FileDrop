//! Device pairing via QR code flow.
//!
//! The pairing process:
//! 1. Laptop generates a one-time token and encodes it with its IP, port,
//!    and certificate fingerprint into a QR code displayed in the terminal
//! 2. Phone scans the QR code, connects via WebSocket to the pairing endpoint
//! 3. Phone sends its certificate + device name
//! 4. Laptop saves the phone's cert in ~/.config/filedrop/peers/
//! 5. Laptop sends its certificate back to the phone
//! 6. Both devices are now paired — future transfers use pinned certs

use anyhow::{Context, Result};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::security::certs;

/// Pairing-related errors
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum PairingError {
    #[error("Pairing timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("Pairing rejected by peer")]
    Rejected,

    #[error("Invalid pairing token")]
    InvalidToken,

    #[error("Certificate exchange failed: {reason}")]
    ExchangeFailed { reason: String },
}

/// Data encoded in the pairing QR code
#[derive(Debug, Serialize, Deserialize)]
pub struct PairingPayload {
    /// IP address of the laptop
    pub ip: String,
    /// Port for the pairing handshake
    pub port: u16,
    /// SHA-256 fingerprint of laptop's certificate
    pub fingerprint: String,
    /// One-time pairing token
    pub token: String,
    /// Device name
    pub device_name: String,
}

/// Message exchanged during pairing
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PairingMessage {
    /// Phone sends its cert and name
    PeerHello {
        device_name: String,
        cert_pem: String,
        fingerprint: String,
    },
    /// Laptop responds with its cert
    PeerAck {
        device_name: String,
        cert_pem: String,
        fingerprint: String,
    },
    /// Pairing successful
    PairingComplete,
    /// Pairing failed
    PairingError {
        reason: String,
    },
}

/// Query parameters for pairing endpoint
#[derive(Debug, Deserialize)]
struct PairingQuery {
    token: String,
}

/// Shared state for the pairing server
struct PairingState {
    expected_token: String,
    our_cert_pem: String,
    our_fingerprint: String,
    our_device_name: String,
    /// Signal that pairing is complete
    done_tx: mpsc::Sender<String>,
}

/// Start the pairing flow — generates QR code and waits for phone to connect
pub async fn start_pairing() -> Result<()> {
    // Ensure certificates exist
    let bundle = certs::ensure_certificates()?;
    let cfg = crate::config::load_config()?;

    // Get local IP address
    let local_ip = get_local_ip().context("Failed to determine local IP address")?;

    // Use a dedicated pairing port (main port + 1)
    let pairing_port = cfg.port + 1;

    // Generate a one-time pairing token
    let token = uuid::Uuid::new_v4().to_string();

    let payload = PairingPayload {
        ip: local_ip.clone(),
        port: pairing_port,
        fingerprint: bundle.fingerprint.clone(),
        token: token.clone(),
        device_name: cfg.device_name.clone(),
    };

    let payload_json = serde_json::to_string(&payload)?;

    // Display the QR code
    print_qr_code(&payload_json)?;

    println!();
    println!("  📱 Scan this QR code with FileDrop on your phone");
    println!();
    println!(
        "  🔒 Fingerprint: {}…",
        &bundle.fingerprint[..20.min(bundle.fingerprint.len())]
    );
    println!("  🌐 Address:     {}:{}", local_ip, pairing_port);
    println!("  🔑 Token:       {}…", &token[..8]);
    println!();

    // Create completion channel
    let (done_tx, mut done_rx) = mpsc::channel::<String>(1);

    // Create pairing state
    let state = Arc::new(PairingState {
        expected_token: token.clone(),
        our_cert_pem: bundle.cert_pem.clone(),
        our_fingerprint: bundle.fingerprint.clone(),
        our_device_name: cfg.device_name.clone(),
        done_tx,
    });

    // Build the pairing server
    let app = Router::new()
        .route("/pair", get(pairing_ws_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], pairing_port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("  ⏳ Waiting for phone to connect... (timeout: 120s)");
    println!("  Press Ctrl+C to cancel");
    println!();

    // Run server with a 120-second timeout
    let server = axum::serve(listener, app);

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                println!("  ❌ Pairing server error: {}", e);
            }
        }
        Some(peer_name) = done_rx.recv() => {
            println!("  ╔══════════════════════════════════════╗");
            println!("  ║       ✅ Pairing Successful!          ║");
            println!("  ╚══════════════════════════════════════╝");
            println!();
            println!("  Paired with: {}", peer_name);
            println!("  You can now run 'filedrop receive' or 'filedrop send'");
            println!();
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
            println!();
            println!("  ⏰ Pairing timed out after 120 seconds");
            println!("  Run 'filedrop pair' to try again");
            println!();
        }
        _ = tokio::signal::ctrl_c() => {
            println!();
            println!("  Pairing cancelled");
        }
    }

    Ok(())
}

/// WebSocket upgrade handler for pairing
async fn pairing_ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<PairingQuery>,
    State(state): State<Arc<PairingState>>,
) -> impl IntoResponse {
    // Validate the one-time token before upgrading
    if query.token != state.expected_token {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "Invalid pairing token",
        )
            .into_response();
    }

    ws.on_upgrade(move |socket| handle_pairing(socket, state))
        .into_response()
}

/// Handle the pairing WebSocket connection — exchange certificates
async fn handle_pairing(mut socket: WebSocket, state: Arc<PairingState>) {
    println!("  🔗 Phone connected! Exchanging certificates...");

    // Wait for the phone's PeerHello
    while let Some(Ok(msg)) = socket.next().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<PairingMessage>(&text) {
                Ok(PairingMessage::PeerHello {
                    device_name,
                    cert_pem,
                    fingerprint,
                }) => {
                    println!(
                        "  📱 Received certificate from: {} ({}…)",
                        device_name,
                        &fingerprint[..16.min(fingerprint.len())]
                    );

                    // Save the peer's certificate
                    match certs::save_peer(&device_name, &cert_pem) {
                        Ok(_) => {
                            println!("  💾 Peer certificate saved");

                            // Send our cert back
                            let ack = PairingMessage::PeerAck {
                                device_name: state.our_device_name.clone(),
                                cert_pem: state.our_cert_pem.clone(),
                                fingerprint: state.our_fingerprint.clone(),
                            };

                            if let Ok(json) = serde_json::to_string(&ack) {
                                let _ = socket.send(Message::Text(json.into())).await;
                            }

                            // Send completion
                            let complete = PairingMessage::PairingComplete;
                            if let Ok(json) = serde_json::to_string(&complete) {
                                let _ = socket.send(Message::Text(json.into())).await;
                            }

                            // Signal the main task
                            let _ = state.done_tx.send(device_name).await;
                        }
                        Err(e) => {
                            println!("  ❌ Failed to save peer cert: {}", e);
                            let err = PairingMessage::PairingError {
                                reason: format!("Failed to save certificate: {}", e),
                            };
                            if let Ok(json) = serde_json::to_string(&err) {
                                let _ = socket.send(Message::Text(json.into())).await;
                            }
                        }
                    }

                    break;
                }
                Ok(_) => {
                    println!("  ⚠ Unexpected pairing message");
                }
                Err(e) => {
                    println!("  ❌ Invalid pairing message: {}", e);
                }
            }
        }
    }
}

/// Generate and print a QR code to the terminal using Unicode block characters
fn print_qr_code(data: &str) -> Result<()> {
    use qrcode::QrCode;

    let code = QrCode::new(data.as_bytes()).context("Failed to generate QR code")?;

    let string = code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(2, 1)
        .build();

    // Print with a nice border
    let lines: Vec<&str> = string.lines().collect();
    let max_width = lines.iter().map(|l| l.len()).max().unwrap_or(0);

    println!();
    println!(
        "  ┌─{}─┐",
        "─".repeat(max_width + 2)
    );
    println!(
        "  │ {:^width$} │",
        "FileDrop Pairing",
        width = max_width + 2
    );
    println!(
        "  ├─{}─┤",
        "─".repeat(max_width + 2)
    );
    for line in &lines {
        println!("  │  {}  │", line);
    }
    println!(
        "  └─{}─┘",
        "─".repeat(max_width + 2)
    );

    Ok(())
}

/// Get the local IP address on the LAN
fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;

    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

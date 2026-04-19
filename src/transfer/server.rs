//! WebSocket server for receive mode.
//!
//! Runs an Axum HTTP/WebSocket server that:
//! - Accepts incoming WebSocket connections from paired phones
//! - Receives files via the FileDrop binary protocol
//! - Writes received chunks to disk using ChunkWriter
//! - Updates the TUI via a channel with transfer progress

use anyhow::Result;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::StreamExt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use crate::transfer::chunker::ChunkWriter;
use crate::transfer::protocol::{
    self, ControlMessage, TransferFile, TransferStatus,
};

/// Events sent from the server to the TUI for display
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Server started listening
    Listening { addr: SocketAddr },
    /// A peer connected
    PeerConnected { name: String, addr: String },
    /// A peer disconnected
    PeerDisconnected { name: String },
    /// A new file transfer started — add to queue
    FileStarted { file: TransferFile },
    /// Progress update on current transfer
    Progress {
        file_name: String,
        bytes_received: u64,
        bytes_total: u64,
        speed: f64,
    },
    /// A file transfer completed
    FileCompleted {
        file_name: String,
        verified: bool,
    },
    /// A file transfer failed
    FileFailed {
        file_name: String,
        error: String,
    },
    /// Log message for the TUI log panel
    Log { message: String },
    /// Error for the TUI log panel
    Error { message: String },
}

/// Shared server state
struct ServerState {
    /// Channel to send events to the TUI
    event_tx: mpsc::UnboundedSender<ServerEvent>,
    /// Output directory for received files
    output_dir: PathBuf,
}

/// Start the WebSocket server on the specified port.
/// This function runs until the server is shut down.
pub async fn start_server(
    port: u16,
    event_tx: mpsc::UnboundedSender<ServerEvent>,
) -> Result<()> {
    let output_dir = std::env::current_dir()?;

    let state = Arc::new(ServerState {
        event_tx: event_tx.clone(),
        output_dir,
    });

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route(
            "/health",
            get(|| async { "FileDrop v0.1.0 OK" }),
        )
        // Serve the embedded web UI for phone browsers
        .route("/", get(crate::web::serve_index))
        .route("/favicon.ico", get(crate::web::serve_favicon))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let _ = event_tx.send(ServerEvent::Listening { addr });
    let _ = event_tx.send(ServerEvent::Log {
        message: format!("WebSocket server listening on ws://0.0.0.0:{}/ws", port),
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_connection(socket, state))
}

/// State for tracking the current file being received
struct ActiveTransfer {
    /// File name being received
    file_name: String,
    /// Expected total size
    total_size: u64,
    /// Chunk writer handling disk I/O and hash verification
    writer: ChunkWriter,
    /// When the transfer started (for speed calculation)
    started_at: Instant,
    /// Last progress report time
    last_report: Instant,
}

/// Handle an individual WebSocket connection — the main receive loop
async fn handle_connection(mut socket: WebSocket, state: Arc<ServerState>) {
    let peer_addr = "peer".to_string();

    let _ = state.event_tx.send(ServerEvent::PeerConnected {
        name: peer_addr.clone(),
        addr: "LAN".to_string(),
    });
    let _ = state.event_tx.send(ServerEvent::Log {
        message: "Device connected".to_string(),
    });

    // Track the current file being received
    let mut active_transfer: Option<ActiveTransfer> = None;

    while let Some(msg_result) = socket.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                let _ = state.event_tx.send(ServerEvent::Error {
                    message: format!("WebSocket error: {}", e),
                });
                break;
            }
        };

        match msg {
            // ── Control messages (JSON text frames) ──────────────────
            Message::Text(text) => {
                match protocol::parse_control_message(&text) {
                    Ok(ctrl) => {
                        match ctrl {
                            ControlMessage::FileStart {
                                name, size, sha256, ..
                            } => {
                                let _ = state.event_tx.send(ServerEvent::Log {
                                    message: format!(
                                        "Incoming: '{}' ({})",
                                        name,
                                        protocol::format_bytes(size)
                                    ),
                                });

                                // Create the transfer file entry for TUI
                                let mut tf = TransferFile::new(
                                    name.clone(),
                                    size,
                                    sha256.clone(),
                                );
                                tf.status = TransferStatus::InProgress;
                                let _ = state.event_tx.send(ServerEvent::FileStarted {
                                    file: tf,
                                });

                                // Open chunk writer on disk
                                let out_path = state.output_dir.join(&name);
                                match ChunkWriter::new(&out_path, size, sha256).await {
                                    Ok(writer) => {
                                        active_transfer = Some(ActiveTransfer {
                                            file_name: name,
                                            total_size: size,
                                            writer,
                                            started_at: Instant::now(),
                                            last_report: Instant::now(),
                                        });
                                    }
                                    Err(e) => {
                                        let _ = state.event_tx.send(ServerEvent::Error {
                                            message: format!(
                                                "Failed to create output file: {}",
                                                e
                                            ),
                                        });
                                        // Send NACK back to sender
                                        let nack = protocol::serialize_control_message(
                                            &ControlMessage::FileAck {
                                                success: false,
                                                error: Some(format!(
                                                    "Cannot write file: {}",
                                                    e
                                                )),
                                            },
                                        );
                                        if let Ok(json) = nack {
                                            let _ = socket
                                                .send(Message::Text(json.into()))
                                                .await;
                                        }
                                    }
                                }
                            }

                            ControlMessage::FileDone { checksum: _ } => {
                                if let Some(transfer) = active_transfer.take() {
                                    let file_name = transfer.file_name.clone();
                                    let elapsed =
                                        transfer.started_at.elapsed().as_secs_f64();
                                    let bytes = transfer.writer.bytes_written();

                                    match transfer.writer.finalize().await {
                                        Ok(true) => {
                                            let _ = state.event_tx.send(
                                                ServerEvent::FileCompleted {
                                                    file_name: file_name.clone(),
                                                    verified: true,
                                                },
                                            );
                                            let _ = state.event_tx.send(ServerEvent::Log {
                                                message: format!(
                                                    "✓ '{}' saved ({} in {:.1}s)",
                                                    file_name,
                                                    protocol::format_bytes(bytes),
                                                    elapsed
                                                ),
                                            });
                                            // Send ACK
                                            let ack = protocol::serialize_control_message(
                                                &ControlMessage::FileAck {
                                                    success: true,
                                                    error: None,
                                                },
                                            );
                                            if let Ok(json) = ack {
                                                let _ = socket
                                                    .send(Message::Text(json.into()))
                                                    .await;
                                            }
                                        }
                                        Ok(false) => {
                                            let _ = state.event_tx.send(
                                                ServerEvent::FileFailed {
                                                    file_name: file_name.clone(),
                                                    error: "Checksum mismatch"
                                                        .to_string(),
                                                },
                                            );
                                            let _ = state.event_tx.send(ServerEvent::Error {
                                                message: format!(
                                                    "✗ '{}' checksum FAILED",
                                                    file_name
                                                ),
                                            });
                                        }
                                        Err(e) => {
                                            let _ = state.event_tx.send(
                                                ServerEvent::FileFailed {
                                                    file_name: file_name.clone(),
                                                    error: e.to_string(),
                                                },
                                            );
                                        }
                                    }
                                }
                            }

                            ControlMessage::Cancel { reason } => {
                                let _ = state.event_tx.send(ServerEvent::Log {
                                    message: format!("Transfer cancelled: {}", reason),
                                });
                                active_transfer = None;
                            }

                            ControlMessage::Ping { timestamp } => {
                                let pong = protocol::serialize_control_message(
                                    &ControlMessage::Pong { timestamp },
                                );
                                if let Ok(json) = pong {
                                    let _ =
                                        socket.send(Message::Text(json.into())).await;
                                }
                            }

                            ControlMessage::BatchStart {
                                file_count,
                                total_size,
                            } => {
                                let _ = state.event_tx.send(ServerEvent::Log {
                                    message: format!(
                                        "Batch: {} files, {} total",
                                        file_count,
                                        protocol::format_bytes(total_size)
                                    ),
                                });
                            }

                            _ => {}
                        }
                    }
                    Err(e) => {
                        let _ = state.event_tx.send(ServerEvent::Error {
                            message: format!("Bad control frame: {}", e),
                        });
                    }
                }
            }

            // ── Binary data chunks ───────────────────────────────────
            Message::Binary(data) => {
                if let Some(ref mut transfer) = active_transfer {
                    match transfer.writer.write_chunk(&data).await {
                        Ok(written) => {
                            // Throttle progress reports to ~10/sec
                            if transfer.last_report.elapsed().as_millis() > 100 {
                                let elapsed =
                                    transfer.started_at.elapsed().as_secs_f64();
                                let speed = if elapsed > 0.0 {
                                    written as f64 / elapsed
                                } else {
                                    0.0
                                };

                                let _ = state.event_tx.send(ServerEvent::Progress {
                                    file_name: transfer.file_name.clone(),
                                    bytes_received: written,
                                    bytes_total: transfer.total_size,
                                    speed,
                                });

                                transfer.last_report = Instant::now();
                            }
                        }
                        Err(e) => {
                            let _ = state.event_tx.send(ServerEvent::Error {
                                message: format!(
                                    "Write error for '{}': {}",
                                    transfer.file_name, e
                                ),
                            });
                        }
                    }
                } else {
                    let _ = state.event_tx.send(ServerEvent::Error {
                        message: "Received binary data without active transfer"
                            .to_string(),
                    });
                }
            }

            Message::Close(_) => {
                let _ = state.event_tx.send(ServerEvent::Log {
                    message: "Connection closed".to_string(),
                });
                break;
            }

            _ => {}
        }
    }

    let _ = state.event_tx.send(ServerEvent::PeerDisconnected {
        name: peer_addr,
    });
}

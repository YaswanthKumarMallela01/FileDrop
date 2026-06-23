//! WebSocket server for receive mode.
//!
//! Runs an Axum HTTP/WebSocket server that:
//! - Accepts incoming WebSocket connections from paired phones
//! - Receives files via the FileDrop binary protocol
//! - Writes received chunks to disk using ChunkWriter
//! - Updates the TUI via a channel with transfer progress
//! - Supports single-device lock (Feature 1)
//! - Supports bidirectional push transfers laptop → phone (Feature 2B)
//! - Serves PWA assets: manifest.json, sw.js, icons (Feature 6)

use anyhow::Result;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use futures_util::StreamExt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};

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

/// Session lock state for single-device mode (Feature 1)
#[derive(Debug)]
struct SessionLock {
    /// Currently connected device IP, if any
    locked_to: Option<SocketAddr>,
}

/// Shared server state
struct ServerState {
    /// Channel to send events to the TUI
    event_tx: mpsc::UnboundedSender<ServerEvent>,
    /// Output directory for received files
    output_dir: PathBuf,
    /// Session lock — None means no lock (multi-mode), Some means single-device
    session_lock: Option<Mutex<SessionLock>>,
    /// Channel for push-file requests from TUI file browser
    push_rx: Mutex<Option<mpsc::UnboundedReceiver<PushRequest>>>,
    /// Whether E2E encryption is enabled
    encrypt: bool,
}

/// A request to push a file from laptop to phone
#[derive(Debug)]
pub struct PushRequest {
    pub name: String,
    pub data: Vec<u8>,
    pub sha256: String,
}

/// Start the WebSocket server on the specified port.
/// This function runs until the server is shut down.
pub async fn start_server(
    port: u16,
    event_tx: mpsc::UnboundedSender<ServerEvent>,
    multi: bool,
    encrypt: bool,
    push_rx: Option<mpsc::UnboundedReceiver<PushRequest>>,
) -> Result<()> {
    let output_dir = std::env::current_dir()?;

    let state = Arc::new(ServerState {
        event_tx: event_tx.clone(),
        output_dir,
        session_lock: if multi {
            None // No lock in multi mode
        } else {
            Some(Mutex::new(SessionLock { locked_to: None }))
        },
        push_rx: Mutex::new(push_rx),
        encrypt,
    });

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route(
            "/health",
            get(|| async { "FileDrop v0.5.3 OK" }),
        )
        // Serve the embedded web UI for phone browsers
        .route("/", get(crate::web::serve_index))
        .route("/favicon.ico", get(crate::web::serve_favicon))
        // PWA routes (Feature 6)
        .route("/manifest.json", get(crate::web::serve_manifest))
        .route("/sw.js", get(crate::web::serve_sw))
        .route("/icon-192.png", get(crate::web::serve_icon_192))
        .route("/icon-512.png", get(crate::web::serve_icon_512))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let _ = event_tx.send(ServerEvent::Listening { addr });
    let _ = event_tx.send(ServerEvent::Log {
        message: format!("WebSocket server listening on ws://0.0.0.0:{}/ws", port),
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let make_service = app.into_make_service_with_connect_info::<SocketAddr>();
    loop {
        let (stream, remote_addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                let _ = event_tx.send(ServerEvent::Error {
                    message: format!("Accept error: {}", e),
                });
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let make_service = make_service.clone();
        tokio::spawn(async move {
            use tower::Service;
            let io = hyper_util::rt::TokioIo::new(stream);
            // Get the service for this connection
            let mut service_maker = make_service;
            let service = match service_maker.call(remote_addr).await {
                Ok(s) => s,
                Err(_) => return,
            };
            let hyper_service = hyper_util::service::TowerToHyperService::new(service);
            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, hyper_service);
            let conn = conn.with_upgrades();
            let _ = conn.await;
        });
    }

    #[allow(unreachable_code)]
    Ok(())
}

/// WebSocket upgrade handler with session lock check (Feature 1)
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
    req: axum::extract::ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let peer_addr = req.0;

    // Check session lock (Feature 1)
    if let Some(ref lock) = state.session_lock {
        let guard = lock.lock().await;
        if let Some(locked_addr) = guard.locked_to {
            if locked_addr != peer_addr {
                // Another device is already connected — reject
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "session_locked",
                        "message": "Another device is already connected."
                    })),
                ).into_response();
            }
        }
    }

    let ws = ws
        .max_frame_size(4 * 1024 * 1024)
        .max_message_size(64 * 1024 * 1024)
        .write_buffer_size(4 * 1024 * 1024)
        .max_write_buffer_size(8 * 1024 * 1024);

    ws.on_upgrade(move |socket| handle_connection(socket, state, peer_addr))
        .into_response()
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
async fn handle_connection(
    mut socket: WebSocket,
    state: Arc<ServerState>,
    peer_addr: SocketAddr,
) {
    // Acquire session lock (Feature 1)
    if let Some(ref lock) = state.session_lock {
        let mut guard = lock.lock().await;
        guard.locked_to = Some(peer_addr);
        let _ = state.event_tx.send(ServerEvent::Log {
            message: format!("[LOCK] Session locked to {}. Further connections rejected.", peer_addr),
        });
    }

    let _ = state.event_tx.send(ServerEvent::PeerConnected {
        name: peer_addr.to_string(),
        addr: peer_addr.ip().to_string(),
    });
    let _ = state.event_tx.send(ServerEvent::Log {
        message: format!("Device connected from {}", peer_addr),
    });

    // Track the current file being received
    let mut active_transfer: Option<ActiveTransfer> = None;

    // Check if we have push requests to forward to this phone
    let mut push_rx = {
        let mut guard = state.push_rx.lock().await;
        guard.take()
    };

    loop {
        // Check for push requests from TUI file browser
        let push_check = async {
            if let Some(ref mut rx) = push_rx {
                rx.recv().await
            } else {
                std::future::pending::<Option<PushRequest>>().await
            }
        };

        tokio::select! {
            // Incoming messages from phone
            msg_opt = socket.next() => {
                let msg = match msg_opt {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => {
                        let _ = state.event_tx.send(ServerEvent::Error {
                            message: format!("WebSocket error: {}", e),
                        });
                        break;
                    }
                    None => break,
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

                                    ControlMessage::FileDone { checksum } => {
                                        if let Some(mut transfer) = active_transfer.take() {
                                            let file_name = transfer.file_name.clone();
                                            let elapsed =
                                                transfer.started_at.elapsed().as_secs_f64();
                                            let bytes = transfer.writer.bytes_written();

                                            // Set the expected hash from the checksum in file_done
                                            transfer.writer.set_expected_hash(checksum);

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
                                                    // Send NACK
                                                    let nack = protocol::serialize_control_message(
                                                        &ControlMessage::FileAck {
                                                            success: false,
                                                            error: Some("Checksum mismatch".to_string()),
                                                        },
                                                    );
                                                    if let Ok(json) = nack {
                                                        let _ = socket
                                                            .send(Message::Text(json.into()))
                                                            .await;
                                                    }
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

                                    ControlMessage::PushAck { success, transfer_id } => {
                                        if success {
                                            let _ = state.event_tx.send(ServerEvent::Log {
                                                message: format!("[PUSH] Transfer {} acknowledged by phone", &transfer_id[..8]),
                                            });
                                        } else {
                                            let _ = state.event_tx.send(ServerEvent::Error {
                                                message: format!("[PUSH] Transfer {} failed on phone side", &transfer_id[..8]),
                                            });
                                        }
                                    }

                                    ControlMessage::KeyExchange { public_key } => {
                                        let _ = state.event_tx.send(ServerEvent::Log {
                                            message: "[ENC] Received phone's ECDH public key".to_string(),
                                        });
                                        // Key exchange handling would be done here in encrypted mode
                                        let _ = public_key; // consumed by encryption setup
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

            // Push requests from TUI file browser (Feature 2B)
            Some(push_req) = push_check => {
                let transfer_id = uuid::Uuid::new_v4().to_string();
                let _ = state.event_tx.send(ServerEvent::Log {
                    message: format!(
                        "[PUSH] Sending '{}' ({}) to phone",
                        push_req.name,
                        protocol::format_bytes(push_req.data.len() as u64)
                    ),
                });

                // Send push_start
                let start_msg = protocol::serialize_control_message(
                    &ControlMessage::PushStart {
                        name: push_req.name.clone(),
                        size: push_req.data.len() as u64,
                        sha256: "streaming".to_string(),
                        transfer_id: transfer_id.clone(),
                    },
                );
                if let Ok(json) = start_msg {
                    let _ = socket.send(Message::Text(json.into())).await;
                }

                // Send binary chunks (4MB each)
                let chunk_size = 4 * 1024 * 1024;
                for chunk in push_req.data.chunks(chunk_size) {
                    let _ = socket.send(Message::Binary(chunk.to_vec().into())).await;
                }

                // Send push_done with checksum
                let done_msg = protocol::serialize_control_message(
                    &ControlMessage::PushDone {
                        checksum: format!("sha256:{}", push_req.sha256),
                        transfer_id,
                    },
                );
                if let Ok(json) = done_msg {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
        }
    }

    // Release session lock on disconnect (Feature 1)
    if let Some(ref lock) = state.session_lock {
        let mut guard = lock.lock().await;
        guard.locked_to = None;
    }

    let _ = state.event_tx.send(ServerEvent::PeerDisconnected {
        name: peer_addr.to_string(),
    });
}

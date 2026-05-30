//! LAN folder sync server and client.
//!
//! Provides real-time one-way file synchronization over WebSocket.
//! - **Source mode**: watches a folder and pushes changes to a listener
//! - **Listener mode**: receives changes and writes them to a save directory
//!
//! Port: 7880 (separate from transfer 7878 and share 7879)

use anyhow::{Context, Result};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::watcher::{self, SyncEvent};

/// Sync mode — source pushes, listener receives
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncMode {
    Source,
    Listener,
}

/// Sync protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncMessage {
    /// File manifest for initial sync comparison
    SyncManifest { files: Vec<SyncFileEntry> },
    /// Start sending a file
    SyncFileStart { path: String, size: u64, sha256: String },
    /// File transfer complete
    SyncFileDone { path: String, checksum: String },
    /// Delete a file on the destination
    SyncDelete { path: String },
    /// Create a directory on the destination
    SyncMkdir { path: String },
    /// Acknowledgment
    SyncAck { success: bool },
}

/// A file entry in the sync manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// Sync status for TUI display
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    Connecting,
    Watching,
    Syncing,
    Idle,
    Error(String),
}

impl std::fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncStatus::Connecting => write!(f, "CONNECTING"),
            SyncStatus::Watching => write!(f, "WATCHING"),
            SyncStatus::Syncing => write!(f, "SYNCING"),
            SyncStatus::Idle => write!(f, "IDLE"),
            SyncStatus::Error(e) => write!(f, "ERROR: {}", e),
        }
    }
}

/// Events sent to the TUI from sync operations
#[derive(Debug, Clone)]
pub enum SyncTuiEvent {
    StatusChanged(SyncStatus),
    Log(String),
    Error(String),
    FileSync { path: String, size: u64, direction: String },
}

const SYNC_PORT: u16 = 7880;
const CHUNK_SIZE: usize = 256 * 1024; // 256KB

/// Build a manifest of all files under a directory (recursive)
async fn build_manifest(root: &Path) -> Result<Vec<SyncFileEntry>> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut read_dir = tokio::fs::read_dir(&dir).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            let meta = entry.metadata().await?;

            if meta.is_file() {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                let rel_str = relative.to_string_lossy().replace('\\', "/");

                // Compute SHA-256
                let data = tokio::fs::read(&path).await?;
                let hash = hex::encode(Sha256::digest(&data));

                entries.push(SyncFileEntry {
                    path: rel_str,
                    size: meta.len(),
                    sha256: hash,
                });
            } else if meta.is_dir() {
                stack.push(path);
            }
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Send a single file over WebSocket in chunks
async fn send_file_ws(
    socket: &mut WebSocket,
    root: &Path,
    relative_path: &str,
    tui_tx: &mpsc::UnboundedSender<SyncTuiEvent>,
) -> Result<()> {
    let file_path = root.join(relative_path);
    let data = tokio::fs::read(&file_path).await
        .with_context(|| format!("Failed to read {}", file_path.display()))?;

    let hash = hex::encode(Sha256::digest(&data));
    let size = data.len() as u64;

    // Send file_start
    let start_msg = serde_json::to_string(&SyncMessage::SyncFileStart {
        path: relative_path.to_string(),
        size,
        sha256: hash.clone(),
    })?;
    socket.send(Message::Text(start_msg.into())).await?;

    // Send binary chunks
    for chunk in data.chunks(CHUNK_SIZE) {
        socket.send(Message::Binary(chunk.to_vec().into())).await?;
    }

    // Send file_done
    let done_msg = serde_json::to_string(&SyncMessage::SyncFileDone {
        path: relative_path.to_string(),
        checksum: format!("sha256:{}", hash),
    })?;
    socket.send(Message::Text(done_msg.into())).await?;

    let _ = tui_tx.send(SyncTuiEvent::FileSync {
        path: relative_path.to_string(),
        size,
        direction: "→ sent".to_string(),
    });

    Ok(())
}

// ── Source Mode ─────────────────────────────────────────────────────────────

/// Run sync in source mode — connect to listener and push changes
pub async fn run_source(
    folder: PathBuf,
    peer_ip: String,
    tui_tx: mpsc::UnboundedSender<SyncTuiEvent>,
) -> Result<()> {
    let _ = tui_tx.send(SyncTuiEvent::StatusChanged(SyncStatus::Connecting));
    let _ = tui_tx.send(SyncTuiEvent::Log(format!(
        "[SYNC] Connecting to {}:{}...", peer_ip, SYNC_PORT
    )));

    let url = format!("ws://{}:{}/sync", peer_ip, SYNC_PORT);
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await
        .context("Failed to connect to sync peer")?;

    let (mut write, mut read) = ws_stream.split();

    let _ = tui_tx.send(SyncTuiEvent::Log("[SYNC] Connected to peer".to_string()));

    // Build and send our manifest
    let _ = tui_tx.send(SyncTuiEvent::StatusChanged(SyncStatus::Syncing));
    let _ = tui_tx.send(SyncTuiEvent::Log("[SYNC] Building file manifest...".to_string()));

    let our_manifest = build_manifest(&folder).await?;
    let manifest_msg = serde_json::to_string(&SyncMessage::SyncManifest {
        files: our_manifest.clone(),
    })?;
    write.send(tokio_tungstenite::tungstenite::Message::Text(manifest_msg.into())).await?;

    // Receive peer's manifest
    let peer_manifest: Vec<SyncFileEntry> = if let Some(Ok(msg)) = read.next().await {
        let text = msg.into_text().unwrap_or_default();
        if let Ok(SyncMessage::SyncManifest { files }) = serde_json::from_str::<SyncMessage>(&text) {
            files
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Diff: find files we have that peer doesn't or that differ
    let peer_map: HashMap<String, String> = peer_manifest
        .iter()
        .map(|f| (f.path.clone(), f.sha256.clone()))
        .collect();

    let mut to_sync: Vec<String> = Vec::new();
    for entry in &our_manifest {
        match peer_map.get(&entry.path) {
            None => to_sync.push(entry.path.clone()),
            Some(hash) if hash != &entry.sha256 => to_sync.push(entry.path.clone()),
            _ => {}
        }
    }

    let _ = tui_tx.send(SyncTuiEvent::Log(format!(
        "[SYNC] Initial sync: {} files to transfer", to_sync.len()
    )));

    // Convert tokio-tungstenite write to axum-compatible messages
    // We need to work with the raw tungstenite types here
    for path in &to_sync {
        let file_path = folder.join(path);
        if let Ok(data) = tokio::fs::read(&file_path).await {
            let hash = hex::encode(Sha256::digest(&data));
            let size = data.len() as u64;

            let start = serde_json::to_string(&SyncMessage::SyncFileStart {
                path: path.clone(),
                size,
                sha256: hash.clone(),
            })?;
            write.send(tokio_tungstenite::tungstenite::Message::Text(start.into())).await?;

            for chunk in data.chunks(CHUNK_SIZE) {
                write.send(tokio_tungstenite::tungstenite::Message::Binary(chunk.to_vec().into())).await?;
            }

            let done = serde_json::to_string(&SyncMessage::SyncFileDone {
                path: path.clone(),
                checksum: format!("sha256:{}", hash),
            })?;
            write.send(tokio_tungstenite::tungstenite::Message::Text(done.into())).await?;

            let _ = tui_tx.send(SyncTuiEvent::FileSync {
                path: path.clone(),
                size,
                direction: "→ sent".to_string(),
            });
        }
    }

    let _ = tui_tx.send(SyncTuiEvent::Log("[SYNC] Initial sync complete".to_string()));
    let _ = tui_tx.send(SyncTuiEvent::StatusChanged(SyncStatus::Watching));

    // Start file watcher
    let (watch_tx, mut watch_rx) = mpsc::unbounded_channel::<SyncEvent>();
    let watch_folder = folder.clone();
    tokio::spawn(async move {
        if let Err(e) = watcher::watch_folder(watch_folder, watch_tx).await {
            tracing::error!("[SYNC] Watcher error: {}", e);
        }
    });

    // React to file changes
    while let Some(event) = watch_rx.recv().await {
        match event {
            SyncEvent::FileCreated { relative_path } | SyncEvent::FileModified { relative_path } => {
                let rel_str = relative_path.to_string_lossy().replace('\\', "/");
                let _ = tui_tx.send(SyncTuiEvent::StatusChanged(SyncStatus::Syncing));
                let _ = tui_tx.send(SyncTuiEvent::Log(format!(
                    "[SYNC] Modified: {}", rel_str
                )));

                let file_path = folder.join(&relative_path);
                if let Ok(data) = tokio::fs::read(&file_path).await {
                    let hash = hex::encode(Sha256::digest(&data));
                    let size = data.len() as u64;

                    let start = serde_json::to_string(&SyncMessage::SyncFileStart {
                        path: rel_str.clone(),
                        size,
                        sha256: hash.clone(),
                    })?;
                    write.send(tokio_tungstenite::tungstenite::Message::Text(start.into())).await?;

                    for chunk in data.chunks(CHUNK_SIZE) {
                        write.send(tokio_tungstenite::tungstenite::Message::Binary(chunk.to_vec().into())).await?;
                    }

                    let done = serde_json::to_string(&SyncMessage::SyncFileDone {
                        path: rel_str.clone(),
                        checksum: format!("sha256:{}", hash),
                    })?;
                    write.send(tokio_tungstenite::tungstenite::Message::Text(done.into())).await?;

                    let _ = tui_tx.send(SyncTuiEvent::FileSync {
                        path: rel_str,
                        size,
                        direction: "→ sent".to_string(),
                    });
                }

                let _ = tui_tx.send(SyncTuiEvent::StatusChanged(SyncStatus::Watching));
            }
            SyncEvent::FileDeleted { relative_path } => {
                let rel_str = relative_path.to_string_lossy().replace('\\', "/");
                let _ = tui_tx.send(SyncTuiEvent::Log(format!(
                    "[SYNC] Deleted: {}", rel_str
                )));
                let msg = serde_json::to_string(&SyncMessage::SyncDelete {
                    path: rel_str,
                })?;
                write.send(tokio_tungstenite::tungstenite::Message::Text(msg.into())).await?;
            }
            SyncEvent::DirCreated { relative_path } => {
                let rel_str = relative_path.to_string_lossy().replace('\\', "/");
                let msg = serde_json::to_string(&SyncMessage::SyncMkdir {
                    path: rel_str,
                })?;
                write.send(tokio_tungstenite::tungstenite::Message::Text(msg.into())).await?;
            }
        }
    }

    Ok(())
}

// ── Listener Mode ───────────────────────────────────────────────────────────

struct ListenerState {
    save_dir: PathBuf,
    tui_tx: mpsc::UnboundedSender<SyncTuiEvent>,
}

/// Run sync in listener mode — start server and receive changes
pub async fn run_listener(
    save_dir: PathBuf,
    tui_tx: mpsc::UnboundedSender<SyncTuiEvent>,
) -> Result<()> {
    // Ensure save directory exists
    tokio::fs::create_dir_all(&save_dir).await?;

    let state = Arc::new(ListenerState {
        save_dir,
        tui_tx: tui_tx.clone(),
    });

    let app = Router::new()
        .route("/sync", get(sync_ws_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], SYNC_PORT));
    let _ = tui_tx.send(SyncTuiEvent::Log(format!(
        "[SYNC] Listener started on port {}", SYNC_PORT
    )));
    let _ = tui_tx.send(SyncTuiEvent::StatusChanged(SyncStatus::Idle));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn sync_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ListenerState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_sync_connection(socket, state))
}

/// Handle an incoming sync connection on the listener side
async fn handle_sync_connection(mut socket: WebSocket, state: Arc<ListenerState>) {
    let _ = state.tui_tx.send(SyncTuiEvent::Log("[SYNC] Peer connected".to_string()));
    let _ = state.tui_tx.send(SyncTuiEvent::StatusChanged(SyncStatus::Syncing));

    // Track active file being received
    struct ActiveReceive {
        path: String,
        data: Vec<u8>,
        expected_size: u64,
        expected_hash: String,
    }
    let mut active: Option<ActiveReceive> = None;

    // Build our manifest for initial comparison
    let our_manifest = match build_manifest(&state.save_dir).await {
        Ok(m) => m,
        Err(e) => {
            let _ = state.tui_tx.send(SyncTuiEvent::Error(format!(
                "[SYNC] Failed to build manifest: {}", e
            )));
            Vec::new()
        }
    };

    while let Some(msg_result) = socket.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                let _ = state.tui_tx.send(SyncTuiEvent::Error(format!(
                    "[SYNC] WebSocket error: {}", e
                )));
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                if let Ok(sync_msg) = serde_json::from_str::<SyncMessage>(&text) {
                    match sync_msg {
                        SyncMessage::SyncManifest { files: _peer_files } => {
                            // Send our manifest back
                            let reply = serde_json::to_string(&SyncMessage::SyncManifest {
                                files: our_manifest.clone(),
                            }).unwrap_or_default();
                            let _ = socket.send(Message::Text(reply.into())).await;
                        }

                        SyncMessage::SyncFileStart { path, size, sha256 } => {
                            let _ = state.tui_tx.send(SyncTuiEvent::Log(format!(
                                "[SYNC] Receiving: {} ({})",
                                path,
                                crate::transfer::protocol::format_bytes(size)
                            )));
                            active = Some(ActiveReceive {
                                path,
                                data: Vec::with_capacity(size as usize),
                                expected_size: size,
                                expected_hash: sha256,
                            });
                        }

                        SyncMessage::SyncFileDone { path, checksum } => {
                            if let Some(recv) = active.take() {
                                let file_path = state.save_dir.join(&recv.path);

                                // Ensure parent directories exist
                                if let Some(parent) = file_path.parent() {
                                    let _ = tokio::fs::create_dir_all(parent).await;
                                }

                                // Verify checksum
                                let hash = hex::encode(Sha256::digest(&recv.data));
                                let expected = checksum.strip_prefix("sha256:").unwrap_or(&checksum);

                                if hash == expected {
                                    if let Err(e) = tokio::fs::write(&file_path, &recv.data).await {
                                        let _ = state.tui_tx.send(SyncTuiEvent::Error(format!(
                                            "[SYNC] Write error for {}: {}", path, e
                                        )));
                                    } else {
                                        let _ = state.tui_tx.send(SyncTuiEvent::FileSync {
                                            path: recv.path,
                                            size: recv.expected_size,
                                            direction: "← received".to_string(),
                                        });
                                    }
                                } else {
                                    let _ = state.tui_tx.send(SyncTuiEvent::Error(format!(
                                        "[SYNC] Checksum mismatch for {}", path
                                    )));
                                }

                                let ack = serde_json::to_string(&SyncMessage::SyncAck {
                                    success: hash == expected,
                                }).unwrap_or_default();
                                let _ = socket.send(Message::Text(ack.into())).await;
                            }
                        }

                        SyncMessage::SyncDelete { path } => {
                            let file_path = state.save_dir.join(&path);
                            let _ = state.tui_tx.send(SyncTuiEvent::Log(format!(
                                "[SYNC] Deleting: {}", path
                            )));
                            if file_path.exists() {
                                let _ = tokio::fs::remove_file(&file_path).await;
                            }
                        }

                        SyncMessage::SyncMkdir { path } => {
                            let dir_path = state.save_dir.join(&path);
                            let _ = tokio::fs::create_dir_all(&dir_path).await;
                        }

                        _ => {}
                    }
                }
            }

            Message::Binary(data) => {
                if let Some(ref mut recv) = active {
                    recv.data.extend_from_slice(&data);
                }
            }

            Message::Close(_) => break,
            _ => {}
        }
    }

    let _ = state.tui_tx.send(SyncTuiEvent::Log("[SYNC] Peer disconnected".to_string()));
    let _ = state.tui_tx.send(SyncTuiEvent::StatusChanged(SyncStatus::Idle));
}

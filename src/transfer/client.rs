//! WebSocket client for send mode.
//!
//! Connects to a paired phone (or laptop in test mode) via WebSocket
//! and sends files using the FileDrop binary protocol:
//! 1. Send FileStart JSON header
//! 2. Stream binary chunk frames (256KB each)
//! 3. Send FileDone JSON with checksum
//! 4. Wait for FileAck

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::transfer::chunker::{self, DEFAULT_CHUNK_SIZE};
use crate::transfer::protocol::{self, ControlMessage, TransferFile, TransferStatus};

/// Events sent from the client to the TUI for display
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ClientEvent {
    /// Connecting to peer
    Connecting { addr: String },
    /// Connected to peer
    Connected { peer_name: String },
    /// Disconnected from peer
    Disconnected,
    /// File transfer started
    FileStarted { file: TransferFile },
    /// Progress update
    Progress {
        file_name: String,
        bytes_sent: u64,
        bytes_total: u64,
        speed: f64,
    },
    /// File transfer completed
    FileCompleted { file_name: String },
    /// File transfer failed
    FileFailed { file_name: String, error: String },
    /// Batch transfer complete
    BatchComplete { successful: usize, failed: usize },
    /// Log message
    Log { message: String },
    /// Error occurred
    Error { message: String },
}

/// Run the send client — connects to a peer and sends files
pub async fn send_files(
    peer_addr: &str,
    files: Vec<PathBuf>,
    event_tx: mpsc::UnboundedSender<ClientEvent>,
) -> Result<()> {
    let _ = event_tx.send(ClientEvent::Connecting {
        addr: peer_addr.to_string(),
    });
    let _ = event_tx.send(ClientEvent::Log {
        message: format!("Connecting to ws://{}...", peer_addr),
    });

    // Connect via WebSocket
    let url = format!("ws://{}/ws", peer_addr);
    let (ws_stream, _response) = connect_async(&url)
        .await
        .with_context(|| format!("Failed to connect to {}", url))?;

    let _ = event_tx.send(ClientEvent::Connected {
        peer_name: peer_addr.to_string(),
    });
    let _ = event_tx.send(ClientEvent::Log {
        message: "Connected!".to_string(),
    });

    let (mut write, mut read) = ws_stream.split();

    // Send batch start if multiple files
    if files.len() > 1 {
        let mut total_size = 0u64;
        for f in &files {
            if let Ok(meta) = tokio::fs::metadata(f).await {
                total_size += meta.len();
            }
        }
        let batch_msg = protocol::serialize_control_message(
            &ControlMessage::BatchStart {
                file_count: files.len(),
                total_size,
            },
        )?;
        write.send(Message::Text(batch_msg.into())).await?;
    }

    let _ = event_tx.send(ClientEvent::Log {
        message: format!("Preparing {} file(s) for transfer...", files.len()),
    });

    let mut successful = 0usize;
    let mut failed = 0usize;

    for file_path in &files {
        match send_single_file(file_path, &mut write, &mut read, &event_tx).await {
            Ok(_) => successful += 1,
            Err(e) => {
                failed += 1;
                let name = file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                let _ = event_tx.send(ClientEvent::FileFailed {
                    file_name: name.to_string(),
                    error: e.to_string(),
                });
                let _ = event_tx.send(ClientEvent::Error {
                    message: format!("Failed to send '{}': {}", name, e),
                });
            }
        }
    }

    // Send batch done
    if files.len() > 1 {
        let batch_done = protocol::serialize_control_message(
            &ControlMessage::BatchDone {
                successful,
                failed,
            },
        )?;
        write.send(Message::Text(batch_done.into())).await?;
    }

    let _ = event_tx.send(ClientEvent::BatchComplete {
        successful,
        failed,
    });

    // Close the WebSocket gracefully
    write.close().await?;

    Ok(())
}

/// Send a single file over the WebSocket connection
async fn send_single_file<S, R>(
    path: &Path,
    write: &mut S,
    read: &mut R,
    event_tx: &mpsc::UnboundedSender<ClientEvent>,
) -> Result<()>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    // Get file metadata and compute hash
    let (name, size) = chunker::file_metadata(path).await?;

    let _ = event_tx.send(ClientEvent::Log {
        message: format!("Hashing '{}'...", name),
    });
    let sha256 = chunker::compute_file_hash(path).await?;

    let _ = event_tx.send(ClientEvent::Log {
        message: format!(
            "'{}' ({}) sha256:{}...",
            name,
            protocol::format_bytes(size),
            &sha256[..12]
        ),
    });

    // Notify TUI: file transfer starting
    let mut tf = TransferFile::new(name.clone(), size, sha256.clone());
    tf.status = TransferStatus::InProgress;
    let _ = event_tx.send(ClientEvent::FileStarted { file: tf });

    // 1. Send FileStart control message
    let file_start = protocol::serialize_control_message(&ControlMessage::FileStart {
        name: name.clone(),
        size,
        sha256: sha256.clone(),
        mime_type: None,
    })?;
    write
        .send(Message::Text(file_start.into()))
        .await
        .context("Failed to send FileStart")?;

    // 2. Stream binary chunks
    let file = tokio::fs::File::open(path).await?;
    let mut reader = tokio::io::BufReader::new(file);
    let mut buffer = vec![0u8; DEFAULT_CHUNK_SIZE];
    let mut bytes_sent = 0u64;
    let started_at = Instant::now();
    let mut last_report = Instant::now();

    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }

        write
            .send(Message::Binary(buffer[..n].to_vec().into()))
            .await
            .context("Failed to send chunk")?;

        bytes_sent += n as u64;

        // Throttled progress reports (~10/sec)
        if last_report.elapsed().as_millis() > 100 {
            let elapsed = started_at.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 {
                bytes_sent as f64 / elapsed
            } else {
                0.0
            };

            let _ = event_tx.send(ClientEvent::Progress {
                file_name: name.clone(),
                bytes_sent,
                bytes_total: size,
                speed,
            });

            last_report = Instant::now();
        }
    }

    // 3. Send FileDone with checksum
    let file_done = protocol::serialize_control_message(&ControlMessage::FileDone {
        checksum: format!("sha256:{}", sha256),
    })?;
    write
        .send(Message::Text(file_done.into()))
        .await
        .context("Failed to send FileDone")?;

    // 4. Wait for FileAck
    let _ = event_tx.send(ClientEvent::Log {
        message: format!("Awaiting ACK for '{}'...", name),
    });

    // Read messages until we get a FileAck (with 30s timeout)
    let ack_timeout = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(ack_timeout, async {
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    if let Ok(ControlMessage::FileAck { success, error }) =
                        protocol::parse_control_message(&text)
                    {
                        return Ok((success, error));
                    }
                }
                Ok(Message::Close(_)) => {
                    return Err(anyhow::anyhow!("Connection closed before ACK"));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("WebSocket error waiting for ACK: {}", e));
                }
                _ => {}
            }
        }
        Err(anyhow::anyhow!("Connection ended without ACK"))
    })
    .await
    {
        Ok(Ok((true, _))) => {
            let elapsed = started_at.elapsed().as_secs_f64();
            let _ = event_tx.send(ClientEvent::FileCompleted {
                file_name: name.clone(),
            });
            let _ = event_tx.send(ClientEvent::Log {
                message: format!(
                    "✓ '{}' sent ({} in {:.1}s)",
                    name,
                    protocol::format_bytes(size),
                    elapsed
                ),
            });
        }
        Ok(Ok((false, error))) => {
            let err_msg = error.unwrap_or_else(|| "Unknown error".to_string());
            return Err(anyhow::anyhow!(
                "Receiver rejected '{}': {}",
                name,
                err_msg
            ));
        }
        Ok(Err(e)) => {
            return Err(e);
        }
        Err(_) => {
            return Err(anyhow::anyhow!("Timed out waiting for ACK for '{}'", name));
        }
    }

    Ok(())
}

/// Discover the peer's address via mDNS
pub async fn discover_peer(timeout_secs: u64) -> Result<String> {
    let peers = crate::discovery::browse_peers(timeout_secs).await?;

    if let Some(peer) = peers.first() {
        Ok(format!("{}:{}", peer.addr, peer.port))
    } else {
        Err(anyhow::anyhow!(
            "No FileDrop peers found on the network. Is the receiver running?"
        ))
    }
}

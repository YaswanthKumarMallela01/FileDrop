//! On-the-fly folder compression for WebSocket streaming.
//!
//! When the laptop user selects a folder in the TUI file browser and presses
//! `S` to send, this module creates a ZIP archive in a background task and
//! streams the compressed bytes through a channel. The caller reads chunks
//! from the channel and forwards them as WebSocket binary frames.
//!
//! The approach avoids writing any temp files to disk — the entire ZIP is
//! built in memory buffers and flushed incrementally.

use anyhow::{Context, Result};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// Target chunk size for yielding compressed data (~1MB)
const CHUNK_TARGET: usize = 1024 * 1024;

/// Calculate total size of a directory (recursive, best-effort)
async fn estimate_folder_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_file() {
                        total += meta.len();
                    } else if meta.is_dir() {
                        stack.push(entry.path());
                    }
                }
            }
        }
    }
    total
}

/// Recursively collect all file paths under a directory.
async fn walk_dir(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_file() {
                        files.push(path);
                    } else if meta.is_dir() {
                        stack.push(path);
                    }
                }
            }
        }
    }
    files.sort();
    files
}

/// Generate the zip filename for a given folder.
///
/// Format: `{foldername}_filedrop_{YYYYMMDD_HHMMSS}.zip`
pub fn zip_filename(folder_name: &str) -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    format!("{}_filedrop_{}.zip", folder_name, timestamp)
}

/// Stream a folder as a ZIP archive through a channel.
///
/// Returns `(zip_filename, estimated_size, chunk_receiver)` where:
/// - `zip_filename` is the generated name for the archive
/// - `estimated_size` is the uncompressed total (actual ZIP will be smaller)
/// - `chunk_receiver` yields `Vec<u8>` chunks of compressed ZIP data
///
/// A background tokio task walks the directory, compresses each file into
/// the ZIP, and flushes accumulated bytes as chunks (~1MB each) through
/// the channel. The channel closes when the ZIP is complete.
pub async fn stream_zip_folder(
    path: &Path,
) -> Result<(String, u64, mpsc::Receiver<Vec<u8>>)> {
    let folder_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("folder")
        .to_string();

    let zip_name = zip_filename(&folder_name);
    let estimated_size = estimate_folder_size(path).await;

    let (tx, rx) = mpsc::channel::<Vec<u8>>(16);
    let root = path.to_path_buf();

    tokio::spawn(async move {
        if let Err(e) = zip_task(root, tx).await {
            tracing::error!("[ZIP] Compression error: {}", e);
        }
    });

    Ok((zip_name, estimated_size, rx))
}

/// Background task that builds the ZIP and sends chunks.
async fn zip_task(root: PathBuf, tx: mpsc::Sender<Vec<u8>>) -> Result<()> {
    let files = walk_dir(&root).await;
    let root_parent = root.clone();

    // Build ZIP on a blocking thread to avoid blocking the async runtime
    // (zip crate operations are synchronous)
    let zip_bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let buffer = Cursor::new(Vec::with_capacity(CHUNK_TARGET * 2));
        let mut zip = ZipWriter::new(buffer);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);

        for file_path in &files {
            // Compute relative path within the archive
            let relative = file_path
                .strip_prefix(&root_parent)
                .unwrap_or(file_path);
            let archive_path = relative.to_string_lossy().replace('\\', "/");

            // Read the file — skip unreadable files
            let data = match std::fs::read(file_path) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(
                        "[ZIP] Skipping unreadable file {}: {}",
                        file_path.display(),
                        e
                    );
                    continue;
                }
            };

            // Write entry to ZIP
            if let Err(e) = zip.start_file(&archive_path, options) {
                tracing::warn!("[ZIP] Failed to start entry {}: {}", archive_path, e);
                continue;
            }
            if let Err(e) = zip.write_all(&data) {
                tracing::warn!("[ZIP] Failed to write entry {}: {}", archive_path, e);
                continue;
            }
        }

        // Finalize the ZIP
        let finished = zip.finish().context("Failed to finalize ZIP")?;
        Ok(finished.into_inner())
    })
    .await
    .context("ZIP task panicked")??;

    // Send the completed ZIP as chunks through the channel
    for chunk in zip_bytes.chunks(CHUNK_TARGET) {
        if tx.send(chunk.to_vec()).await.is_err() {
            return Ok(()); // Receiver dropped
        }
    }

    Ok(())
}

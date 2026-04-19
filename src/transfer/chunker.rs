//! File chunking and reassembly for transfer.
//!
//! Handles splitting files into 256KB chunks for sending,
//! and reassembling received chunks back into files.
//! Includes SHA-256 hash verification.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

/// Default chunk size: 256KB
pub const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;

/// Chunker errors
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum ChunkerError {
    #[error("File too large: {size} bytes (max: {max})")]
    FileTooLarge { size: u64, max: u64 },

    #[error("Checksum verification failed")]
    ChecksumFailed,

    #[error("Write error: {0}")]
    WriteError(String),
}

/// Compute SHA-256 hash of a file
pub async fn compute_file_hash(path: &Path) -> Result<String> {
    let file = File::open(path)
        .await
        .with_context(|| format!("Failed to open file for hashing: {}", path.display()))?;

    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; DEFAULT_CHUNK_SIZE];

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .await
            .context("Failed to read file during hashing")?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    let hash = hasher.finalize();
    Ok(hex::encode(hash))
}

/// Read the next chunk from a file at the given offset.
/// Returns None when EOF is reached.
#[allow(dead_code)]
pub async fn read_chunk(
    file: &mut BufReader<File>,
    chunk_size: usize,
) -> Result<Option<Vec<u8>>> {
    let mut buffer = vec![0u8; chunk_size];
    let bytes_read = file
        .read(&mut buffer)
        .await
        .context("Failed to read chunk from file")?;

    if bytes_read == 0 {
        return Ok(None);
    }

    buffer.truncate(bytes_read);
    Ok(Some(buffer))
}

/// A file writer that reassembles chunks and verifies the final checksum.
#[allow(dead_code)]
pub struct ChunkWriter {
    writer: BufWriter<File>,
    hasher: Sha256,
    bytes_written: u64,
    expected_size: u64,
    expected_hash: String,
}

#[allow(dead_code)]
impl ChunkWriter {
    /// Create a new chunk writer for the given output path
    pub async fn new(
        output_path: &Path,
        expected_size: u64,
        expected_hash: String,
    ) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = File::create(output_path)
            .await
            .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;

        Ok(Self {
            writer: BufWriter::new(file),
            hasher: Sha256::new(),
            bytes_written: 0,
            expected_size,
            expected_hash,
        })
    }

    /// Write a chunk of data
    pub async fn write_chunk(&mut self, data: &[u8]) -> Result<u64> {
        self.writer
            .write_all(data)
            .await
            .context("Failed to write chunk")?;
        self.hasher.update(data);
        self.bytes_written += data.len() as u64;

        Ok(self.bytes_written)
    }

    /// Finalize the write, flush to disk, and verify checksum
    pub async fn finalize(mut self) -> Result<bool> {
        self.writer.flush().await.context("Failed to flush file")?;

        let hash = hex::encode(self.hasher.finalize());

        // Strip "sha256:" prefix if present in expected hash
        let expected = self
            .expected_hash
            .strip_prefix("sha256:")
            .unwrap_or(&self.expected_hash);

        if hash != expected {
            tracing::error!(
                "Checksum mismatch: expected {}, got {}",
                expected,
                hash
            );
            return Ok(false);
        }

        tracing::info!(
            "File verified: {} bytes, checksum OK",
            self.bytes_written
        );
        Ok(true)
    }

    /// Get the number of bytes written so far
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// Get the expected total size
    pub fn expected_size(&self) -> u64 {
        self.expected_size
    }

    /// Get transfer progress as a ratio (0.0 - 1.0)
    pub fn progress(&self) -> f64 {
        if self.expected_size == 0 {
            return 1.0;
        }
        self.bytes_written as f64 / self.expected_size as f64
    }
}

/// Get metadata for a file to send
pub async fn file_metadata(path: &Path) -> Result<(String, u64)> {
    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("Failed to read metadata for {}", path.display()))?;

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok((name, metadata.len()))
}

/// Collect all files from a directory (non-recursive)
pub async fn collect_files(path: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();

    if path.is_file() {
        files.push(path.to_path_buf());
    } else if path.is_dir() {
        let mut entries = tokio::fs::read_dir(path)
            .await
            .with_context(|| format!("Failed to read directory: {}", path.display()))?;

        while let Some(entry) = entries.next_entry().await? {
            let entry_path = entry.path();
            if entry_path.is_file() {
                files.push(entry_path);
            }
        }
    }

    Ok(files)
}

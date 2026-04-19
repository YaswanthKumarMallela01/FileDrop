//! Wire protocol types for the FileDrop transfer protocol.
//!
//! The protocol uses WebSocket frames:
//! - Control frames: JSON-encoded messages (text frames)
//! - Data frames: Raw binary chunks (binary frames)
//!
//! Transfer flow:
//! 1. Sender sends `FileStart` with metadata
//! 2. Sender sends binary chunk frames (256KB each)
//! 3. Sender sends `FileDone` with final checksum
//! 4. Receiver verifies checksum and sends `FileAck`

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Protocol-level errors
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum ProtocolError {
    #[error("Invalid message type: {0}")]
    InvalidMessageType(String),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Invalid frame: {reason}")]
    InvalidFrame { reason: String },

    #[error("Transfer cancelled by peer")]
    Cancelled,

    #[error("Transfer failed: {reason}")]
    TransferFailed { reason: String },

    #[error("Unexpected message: expected {expected}, got {actual}")]
    UnexpectedMessage { expected: String, actual: String },
}

/// Control message types sent as JSON text frames
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    /// Announce a file transfer is starting
    FileStart {
        /// Original file name
        name: String,
        /// File size in bytes
        size: u64,
        /// SHA-256 hash of the entire file
        sha256: String,
        /// MIME type if known
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },

    /// Announce a file transfer is complete
    FileDone {
        /// Verification checksum in format "sha256:<hex>"
        checksum: String,
    },

    /// Acknowledge receipt of a complete file
    FileAck {
        /// Whether the file was received successfully
        success: bool,
        /// Error message if not successful
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// Progress update from receiver to sender
    Progress {
        /// Bytes received so far
        bytes_received: u64,
        /// Total bytes expected
        bytes_total: u64,
    },

    /// Request to cancel the current transfer
    Cancel {
        /// Reason for cancellation
        reason: String,
    },

    /// Batch transfer start — multiple files
    BatchStart {
        /// Total number of files in the batch
        file_count: usize,
        /// Total size of all files in bytes
        total_size: u64,
    },

    /// Batch transfer complete
    BatchDone {
        /// Number of files successfully transferred
        successful: usize,
        /// Number of files that failed
        failed: usize,
    },

    /// Ping/keep-alive
    Ping {
        /// Timestamp in milliseconds
        timestamp: u64,
    },

    /// Pong response to ping
    Pong {
        /// Original timestamp echoed back
        timestamp: u64,
    },
}

/// Represents a file being transferred
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TransferFile {
    /// Original file name
    pub name: String,
    /// File size in bytes
    pub size: u64,
    /// SHA-256 hash of the file
    pub sha256: String,
    /// MIME type if known
    pub mime_type: Option<String>,
    /// Bytes transferred so far
    pub bytes_transferred: u64,
    /// Current transfer status
    pub status: TransferStatus,
}

/// Status of a file transfer
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TransferStatus {
    /// Waiting in queue
    Queued,
    /// Currently being transferred
    InProgress,
    /// Transfer completed successfully
    Completed,
    /// Transfer failed with an error
    Failed(String),
    /// Transfer was cancelled
    Cancelled,
}

#[allow(dead_code)]
impl TransferFile {
    /// Create a new transfer file entry
    pub fn new(name: String, size: u64, sha256: String) -> Self {
        Self {
            name,
            size,
            sha256,
            mime_type: None,
            bytes_transferred: 0,
            status: TransferStatus::Queued,
        }
    }

    /// Calculate transfer progress as a percentage (0.0 - 1.0)
    pub fn progress(&self) -> f64 {
        if self.size == 0 {
            return 1.0;
        }
        self.bytes_transferred as f64 / self.size as f64
    }

    /// Get a human-readable size string
    pub fn size_display(&self) -> String {
        format_bytes(self.size)
    }

    /// Get a human-readable transferred size string
    pub fn transferred_display(&self) -> String {
        format_bytes(self.bytes_transferred)
    }
}

/// Format bytes into a human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format transfer speed in bytes/sec to a human-readable string
pub fn format_speed(bytes_per_sec: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;

    if bytes_per_sec >= GB {
        format!("{:.1} GB/s", bytes_per_sec / GB)
    } else if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec / MB)
    } else if bytes_per_sec >= KB {
        format!("{:.0} KB/s", bytes_per_sec / KB)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

/// Parse a control message from a JSON string
pub fn parse_control_message(json: &str) -> Result<ControlMessage, ProtocolError> {
    serde_json::from_str(json).map_err(|e| ProtocolError::InvalidFrame {
        reason: format!("Failed to parse control message: {}", e),
    })
}

/// Serialize a control message to a JSON string
pub fn serialize_control_message(msg: &ControlMessage) -> Result<String, ProtocolError> {
    serde_json::to_string(msg).map_err(|e| ProtocolError::InvalidFrame {
        reason: format!("Failed to serialize control message: {}", e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn test_file_start_serialization() {
        let msg = ControlMessage::FileStart {
            name: "photo.jpg".to_string(),
            size: 2_000_000,
            sha256: "abc123".to_string(),
            mime_type: Some("image/jpeg".to_string()),
        };

        let json = serialize_control_message(&msg).unwrap();
        let parsed = parse_control_message(&json).unwrap();

        match parsed {
            ControlMessage::FileStart { name, size, .. } => {
                assert_eq!(name, "photo.jpg");
                assert_eq!(size, 2_000_000);
            }
            _ => panic!("Expected FileStart"),
        }
    }

    #[test]
    fn test_transfer_file_progress() {
        let mut file = TransferFile::new("test.txt".to_string(), 1000, "hash".to_string());
        assert_eq!(file.progress(), 0.0);

        file.bytes_transferred = 500;
        assert!((file.progress() - 0.5).abs() < f64::EPSILON);

        file.bytes_transferred = 1000;
        assert!((file.progress() - 1.0).abs() < f64::EPSILON);
    }
}

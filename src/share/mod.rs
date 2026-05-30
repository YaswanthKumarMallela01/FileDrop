//! Ephemeral file sharing via one-time links.
//!
//! Provides a standalone HTTP server for `filedrop share <file> [options]`,
//! generating a temporary download link with QR code, optional PIN protection,
//! and configurable expiry.

pub mod server;

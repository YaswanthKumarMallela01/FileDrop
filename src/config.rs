//! Configuration management for FileDrop.
//!
//! Handles loading and saving the application config from
//! `~/.config/filedrop/config.toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application configuration stored in config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Device name shown to peers during pairing
    #[serde(default = "default_device_name")]
    pub device_name: String,

    /// Port to listen on for WebSocket connections
    #[serde(default = "default_port")]
    pub port: u16,

    /// Chunk size in bytes for file transfer (default 256KB)
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,

    /// Whether to enable mDNS discovery
    #[serde(default = "default_true")]
    pub mdns_enabled: bool,

    /// Directory for received files (empty = CWD)
    #[serde(default)]
    pub receive_dir: Option<String>,

    /// TUI color theme
    #[serde(default)]
    pub theme: ThemeConfig,
}

/// TUI theme configuration — colors pulled from Stitch design system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// Primary accent color (cyan)
    #[serde(default = "default_primary_color")]
    pub primary: String,

    /// Background color
    #[serde(default = "default_bg_color")]
    pub background: String,

    /// Surface container color
    #[serde(default = "default_surface_color")]
    pub surface: String,

    /// Error color
    #[serde(default = "default_error_color")]
    pub error: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device_name: default_device_name(),
            port: default_port(),
            chunk_size: default_chunk_size(),
            mdns_enabled: true,
            receive_dir: None,
            theme: ThemeConfig::default(),
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            primary: default_primary_color(),
            background: default_bg_color(),
            surface: default_surface_color(),
            error: default_error_color(),
        }
    }
}

fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "FileDrop-Laptop".to_string())
}

fn default_port() -> u16 {
    7878
}

fn default_chunk_size() -> usize {
    256 * 1024 // 256KB
}

fn default_true() -> bool {
    true
}

// Stitch design system colors
fn default_primary_color() -> String {
    "#00D4FF".to_string()
}

fn default_bg_color() -> String {
    "#111319".to_string()
}

fn default_surface_color() -> String {
    "#191B22".to_string()
}

fn default_error_color() -> String {
    "#FFB4AB".to_string()
}

/// Get the FileDrop config directory path (~/.config/filedrop/)
pub fn config_dir() -> Result<PathBuf> {
    let dir = directories::ProjectDirs::from("", "", "filedrop")
        .context("Failed to determine config directory")?;
    let config_path = dir.config_dir().to_path_buf();
    Ok(config_path)
}

/// Get the path to the config file
pub fn config_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

/// Get the peers directory path
pub fn peers_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("peers"))
}

/// Get the certificates directory path
pub fn certs_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("certs"))
}

/// Load configuration from disk, creating defaults if it doesn't exist
pub fn load_config() -> Result<Config> {
    let path = config_file_path()?;

    if path.exists() {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;
        Ok(config)
    } else {
        let config = Config::default();
        save_config(&config)?;
        Ok(config)
    }
}

/// Save configuration to disk
pub fn save_config(config: &Config) -> Result<()> {
    let path = config_file_path()?;

    // Ensure parent directories exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    let content = toml::to_string_pretty(config).context("Failed to serialize config")?;
    fs::write(&path, content)
        .with_context(|| format!("Failed to write config to {}", path.display()))?;

    tracing::debug!("Config saved to {}", path.display());
    Ok(())
}

/// Ensure all required directories exist
pub fn ensure_directories() -> Result<()> {
    let dirs = [config_dir()?, peers_dir()?, certs_dir()?];

    for dir in &dirs {
        fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
    }

    Ok(())
}

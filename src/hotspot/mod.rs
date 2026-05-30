//! Hotspot setup module — OS detection, connection mode selection,
//! and platform-specific hotspot creation/management.
//!
//! This module provides:
//! - [`ConnectionMode`] enum for choosing between Router (LAN) and Hotspot modes
//! - OS detection via [`detect_os`]
//! - Platform-specific hotspot setup instructions via [`hotspot_instructions`]
//! - Automated Linux hotspot creation via [`auto_create_hotspot`] (uses `nmcli`)
//! - Cleanup instructions via [`cleanup_instructions`]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default SSID used when auto-creating a hotspot on Linux.
const DEFAULT_SSID: &str = "FileDrop";

/// Default password used when auto-creating a hotspot on Linux.
const DEFAULT_PASSWORD: &str = "filedrop123";

/// Default wireless interface name used on Linux.
const DEFAULT_WIFI_IFACE: &str = "wlan0";

// ─── Connection Mode ─────────────────────────────────────────────────────────

/// Connection mode chosen by the user.
///
/// Determines how the laptop and phone establish a network link:
/// - **Router**: Both devices are on the same Wi-Fi network (home router, office LAN, etc.)
/// - **Hotspot**: The laptop creates its own Wi-Fi hotspot for the phone to join.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionMode {
    /// Both devices are on the same existing network (router/LAN).
    Router,
    /// The laptop creates a Wi-Fi hotspot for the phone to connect to.
    Hotspot,
}

impl ConnectionMode {
    /// Returns a lowercase string representation of the mode.
    ///
    /// # Examples
    /// ```
    /// use filedrop::hotspot::ConnectionMode;
    /// assert_eq!(ConnectionMode::Router.as_str(), "router");
    /// assert_eq!(ConnectionMode::Hotspot.as_str(), "hotspot");
    /// ```
    pub fn as_str(&self) -> &str {
        match self {
            ConnectionMode::Router => "router",
            ConnectionMode::Hotspot => "hotspot",
        }
    }

    /// Parses a connection mode from a string (case-insensitive).
    ///
    /// Accepts "router", "lan" → [`ConnectionMode::Router`],
    /// and "hotspot" → [`ConnectionMode::Hotspot`].
    /// Returns `None` for unrecognized input.
    ///
    /// # Examples
    /// ```
    /// use filedrop::hotspot::ConnectionMode;
    /// assert_eq!(ConnectionMode::from_str("Router"), Some(ConnectionMode::Router));
    /// assert_eq!(ConnectionMode::from_str("LAN"), Some(ConnectionMode::Router));
    /// assert_eq!(ConnectionMode::from_str("hotspot"), Some(ConnectionMode::Hotspot));
    /// assert_eq!(ConnectionMode::from_str("bluetooth"), None);
    /// ```
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "router" | "lan" => Some(ConnectionMode::Router),
            "hotspot" => Some(ConnectionMode::Hotspot),
            _ => None,
        }
    }
}

impl std::fmt::Display for ConnectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ─── OS Detection ────────────────────────────────────────────────────────────

/// Detects the current operating system.
///
/// Returns one of `"windows"`, `"macos"`, or `"linux"` based on
/// [`std::env::consts::OS`]. Unknown platforms are returned as-is
/// (e.g. `"freebsd"`).
///
/// # Examples
/// ```
/// let os = filedrop::hotspot::detect_os();
/// assert!(["windows", "macos", "linux"].contains(&os));
/// ```
pub fn detect_os() -> &'static str {
    std::env::consts::OS
}

// ─── Hotspot Instructions ────────────────────────────────────────────────────

/// Returns platform-specific instructions for creating a Wi-Fi hotspot.
///
/// Each returned [`String`] is a single line/step suitable for display in
/// the TUI hotspot guide screen. Instructions cover both GUI and CLI
/// approaches where available.
///
/// # Arguments
/// * `os` — Operating system identifier (as returned by [`detect_os`]).
///
/// # Supported Platforms
/// - `"windows"` — Settings GUI + `netsh` CLI fallback
/// - `"macos"` — System Settings GUI (Internet Sharing)
/// - `"linux"` — `nmcli` one-liner
///
/// Unknown platforms receive a generic fallback message.
pub fn hotspot_instructions(os: &str) -> Vec<String> {
    match os {
        "windows" => vec![
            "=== Windows Hotspot Setup ===".into(),
            "".into(),
            "Option 1 — Settings GUI:".into(),
            "  1. Open Settings (Win + I)".into(),
            "  2. Go to Network & Internet → Mobile Hotspot".into(),
            "  3. Set the network name and password".into(),
            "  4. Toggle \"Mobile Hotspot\" to On".into(),
            "  5. Connect your phone to the hotspot network".into(),
            "".into(),
            "Option 2 — Command Line (Admin CMD/PowerShell):".into(),
            "  1. netsh wlan set hostednetwork mode=allow ssid=FileDrop key=filedrop123".into(),
            "  2. netsh wlan start hostednetwork".into(),
            "  3. Connect your phone to \"FileDrop\" with password \"filedrop123\"".into(),
            "".into(),
            "Note: Hosted network requires a compatible Wi-Fi adapter.".into(),
        ],
        "macos" => vec![
            "=== macOS Hotspot Setup ===".into(),
            "".into(),
            "  1. Open System Settings (Apple menu → System Settings)".into(),
            "  2. Go to General → Sharing".into(),
            "  3. Click \"Internet Sharing\" in the service list".into(),
            "  4. Share your connection from: Ethernet / Thunderbolt".into(),
            "  5. To devices using: Wi-Fi".into(),
            "  6. Click \"Wi-Fi Options...\" to set network name and password".into(),
            "     • Network Name: FileDrop".into(),
            "     • Security: WPA2/WPA3 Personal".into(),
            "     • Password: filedrop123".into(),
            "  7. Toggle Internet Sharing to On".into(),
            "  8. Connect your phone to the \"FileDrop\" network".into(),
        ],
        "linux" => vec![
            "=== Linux Hotspot Setup ===".into(),
            "".into(),
            "  Run the following command:".into(),
            format!(
                "    nmcli device wifi hotspot ssid {} ifname {} password {}",
                DEFAULT_SSID, DEFAULT_WIFI_IFACE, DEFAULT_PASSWORD
            ),
            "".into(),
            "  Or let FileDrop create it automatically (select 'Auto-create' in the menu).".into(),
            "".into(),
            format!("  Connect your phone to \"{}\" with password \"{}\".", DEFAULT_SSID, DEFAULT_PASSWORD),
            "".into(),
            "  Note: Requires NetworkManager and a Wi-Fi adapter that supports AP mode.".into(),
        ],
        other => vec![
            format!("=== {} Hotspot Setup ===", other),
            "".into(),
            "  Automatic hotspot creation is not supported on this platform.".into(),
            "  Please manually create a Wi-Fi hotspot or use Router mode instead.".into(),
        ],
    }
}

// ─── Auto-Create Hotspot (Linux) ─────────────────────────────────────────────

/// Automatically creates a Wi-Fi hotspot on Linux using `nmcli`.
///
/// Runs the command:
/// ```text
/// nmcli device wifi hotspot ssid FileDrop ifname wlan0 password filedrop123
/// ```
///
/// # Returns
/// A tuple of `(ssid, password)` on success.
///
/// # Errors
/// - Returns an error on non-Linux platforms.
/// - Returns an error if `nmcli` is not found or the command fails.
/// - Returns an error if the Wi-Fi adapter doesn't support AP mode.
///
/// # Example
/// ```no_run
/// # async fn example() -> anyhow::Result<()> {
/// let (ssid, password) = filedrop::hotspot::auto_create_hotspot().await?;
/// println!("Hotspot '{}' created with password '{}'", ssid, password);
/// # Ok(())
/// # }
/// ```
pub async fn auto_create_hotspot() -> Result<(String, String)> {
    let os = detect_os();
    if os != "linux" {
        anyhow::bail!(
            "[HOTSPOT] Auto-create is only supported on Linux (current OS: {}). \
             Please create a hotspot manually.",
            os
        );
    }

    tracing::info!(
        "[HOTSPOT] Creating hotspot: ssid={} iface={} password={}",
        DEFAULT_SSID,
        DEFAULT_WIFI_IFACE,
        DEFAULT_PASSWORD
    );

    let output = tokio::process::Command::new("nmcli")
        .args([
            "device",
            "wifi",
            "hotspot",
            "ssid",
            DEFAULT_SSID,
            "ifname",
            DEFAULT_WIFI_IFACE,
            "password",
            DEFAULT_PASSWORD,
        ])
        .output()
        .await
        .context("[HOTSPOT] Failed to execute nmcli — is NetworkManager installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "[HOTSPOT] nmcli hotspot creation failed (exit code: {}).\n\
             stdout: {}\n\
             stderr: {}",
            output.status.code().unwrap_or(-1),
            stdout.trim(),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    tracing::info!("[HOTSPOT] Hotspot created successfully: {}", stdout.trim());

    Ok((DEFAULT_SSID.to_string(), DEFAULT_PASSWORD.to_string()))
}

// ─── Cleanup Instructions ────────────────────────────────────────────────────

/// Returns an OS-specific command to tear down a previously created hotspot.
///
/// Currently only Linux (NetworkManager) has a reliable CLI cleanup path.
/// Returns `None` on platforms where manual cleanup is required.
///
/// # Arguments
/// * `os` — Operating system identifier (as returned by [`detect_os`]).
///
/// # Examples
/// ```
/// use filedrop::hotspot::cleanup_instructions;
/// assert_eq!(
///     cleanup_instructions("linux"),
///     Some("nmcli connection delete FileDrop".to_string()),
/// );
/// assert_eq!(cleanup_instructions("windows"), None);
/// ```
pub fn cleanup_instructions(os: &str) -> Option<String> {
    match os {
        "linux" => Some(format!("nmcli connection delete {}", DEFAULT_SSID)),
        _ => None,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_mode_as_str() {
        assert_eq!(ConnectionMode::Router.as_str(), "router");
        assert_eq!(ConnectionMode::Hotspot.as_str(), "hotspot");
    }

    #[test]
    fn test_connection_mode_from_str_valid() {
        assert_eq!(ConnectionMode::from_str("router"), Some(ConnectionMode::Router));
        assert_eq!(ConnectionMode::from_str("Router"), Some(ConnectionMode::Router));
        assert_eq!(ConnectionMode::from_str("ROUTER"), Some(ConnectionMode::Router));
        assert_eq!(ConnectionMode::from_str("lan"), Some(ConnectionMode::Router));
        assert_eq!(ConnectionMode::from_str("LAN"), Some(ConnectionMode::Router));
        assert_eq!(ConnectionMode::from_str("hotspot"), Some(ConnectionMode::Hotspot));
        assert_eq!(ConnectionMode::from_str("Hotspot"), Some(ConnectionMode::Hotspot));
        assert_eq!(ConnectionMode::from_str("HOTSPOT"), Some(ConnectionMode::Hotspot));
    }

    #[test]
    fn test_connection_mode_from_str_invalid() {
        assert_eq!(ConnectionMode::from_str("bluetooth"), None);
        assert_eq!(ConnectionMode::from_str(""), None);
        assert_eq!(ConnectionMode::from_str("wifi"), None);
    }

    #[test]
    fn test_connection_mode_display() {
        assert_eq!(format!("{}", ConnectionMode::Router), "router");
        assert_eq!(format!("{}", ConnectionMode::Hotspot), "hotspot");
    }

    #[test]
    fn test_connection_mode_serde_roundtrip() {
        let router = ConnectionMode::Router;
        let json = serde_json::to_string(&router).unwrap();
        let deserialized: ConnectionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(router, deserialized);

        let hotspot = ConnectionMode::Hotspot;
        let json = serde_json::to_string(&hotspot).unwrap();
        let deserialized: ConnectionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(hotspot, deserialized);
    }

    #[test]
    fn test_detect_os_returns_known_value() {
        let os = detect_os();
        // Should be one of the compile-target OS values
        assert!(!os.is_empty(), "OS string should not be empty");
    }

    #[test]
    fn test_hotspot_instructions_windows() {
        let instructions = hotspot_instructions("windows");
        assert!(!instructions.is_empty());
        assert!(instructions[0].contains("Windows"));
        // Should mention both GUI and CLI approaches
        let joined = instructions.join("\n");
        assert!(joined.contains("Settings"));
        assert!(joined.contains("netsh"));
    }

    #[test]
    fn test_hotspot_instructions_macos() {
        let instructions = hotspot_instructions("macos");
        assert!(!instructions.is_empty());
        assert!(instructions[0].contains("macOS"));
        let joined = instructions.join("\n");
        assert!(joined.contains("Internet Sharing"));
    }

    #[test]
    fn test_hotspot_instructions_linux() {
        let instructions = hotspot_instructions("linux");
        assert!(!instructions.is_empty());
        assert!(instructions[0].contains("Linux"));
        let joined = instructions.join("\n");
        assert!(joined.contains("nmcli"));
        assert!(joined.contains(DEFAULT_SSID));
        assert!(joined.contains(DEFAULT_PASSWORD));
    }

    #[test]
    fn test_hotspot_instructions_unknown_os() {
        let instructions = hotspot_instructions("freebsd");
        assert!(!instructions.is_empty());
        assert!(instructions[0].contains("freebsd"));
        let joined = instructions.join("\n");
        assert!(joined.contains("not supported"));
    }

    #[test]
    fn test_cleanup_instructions_linux() {
        let cmd = cleanup_instructions("linux");
        assert!(cmd.is_some());
        let cmd = cmd.unwrap();
        assert!(cmd.contains("nmcli"));
        assert!(cmd.contains("delete"));
        assert!(cmd.contains(DEFAULT_SSID));
    }

    #[test]
    fn test_cleanup_instructions_non_linux() {
        assert!(cleanup_instructions("windows").is_none());
        assert!(cleanup_instructions("macos").is_none());
        assert!(cleanup_instructions("freebsd").is_none());
    }

    #[tokio::test]
    async fn test_auto_create_hotspot_non_linux() {
        // On non-Linux platforms (CI, dev machines), this should return an error
        if detect_os() != "linux" {
            let result = auto_create_hotspot().await;
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("only supported on Linux"));
        }
    }
}

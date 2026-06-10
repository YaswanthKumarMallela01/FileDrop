//! Hotspot setup module — OS detection, connection mode selection,
//! and platform-specific hotspot creation/management.
//!
//! This module provides:
//! - [`ConnectionMode`] enum for choosing between Router (LAN) and Hotspot modes
//! - OS detection via [`detect_os`]
//! - Platform-specific hotspot setup instructions via [`hotspot_instructions`]
//! - Phone hotspot detection via [`detect_phone_hotspot`]
//! - Automated Linux hotspot creation via [`auto_create_hotspot`] (uses `nmcli`)
//! - Windows hotspot creation via [`auto_create_hotspot_windows`] (uses `netsh`, best-effort)
//! - Cleanup instructions via [`cleanup_instructions`]
#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default SSID used when auto-creating a hotspot.
const DEFAULT_SSID: &str = "FileDrop";

/// Default password used when auto-creating a hotspot.
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
    pub fn as_str(&self) -> &str {
        match self {
            ConnectionMode::Router => "router",
            ConnectionMode::Hotspot => "hotspot",
        }
    }

    /// Parses a connection mode from a string (case-insensitive).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "router" | "lan" => Some(ConnectionMode::Router),
            "hotspot" | "offline" => Some(ConnectionMode::Hotspot),
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
pub fn detect_os() -> &'static str {
    std::env::consts::OS
}

// ─── Phone Hotspot Detection ─────────────────────────────────────────────────

/// Known phone hotspot IP ranges.
/// - Android default: 192.168.43.x
/// - iOS default:     172.20.10.x
/// - Windows Mobile Hotspot: 192.168.137.x
/// - Some Android variants: 192.168.49.x (Wi-Fi Direct)
const PHONE_HOTSPOT_PREFIXES: &[&str] = &[
    "192.168.43.",  // Android default hotspot
    "172.20.10.",   // iOS default hotspot
    "192.168.137.", // Windows Mobile Hotspot
    "192.168.49.",  // Android Wi-Fi Direct
];

/// Result of phone hotspot detection.
#[derive(Debug, Clone)]
pub struct HotspotDetection {
    /// Whether a phone hotspot network was detected.
    pub detected: bool,
    /// The IP address on the hotspot network (if detected).
    pub ip: Option<String>,
    /// Description of the detected network type.
    pub network_type: Option<String>,
}

/// Checks if the computer is currently connected to a phone hotspot network.
///
/// Scans all network interfaces for IPs in known phone hotspot ranges
/// (Android 192.168.43.x, iOS 172.20.10.x, etc.)
pub fn detect_phone_hotspot() -> HotspotDetection {
    // Try to detect by scanning local IPs
    let ips = get_all_local_ips();

    for ip in &ips {
        if ip.starts_with("192.168.43.") {
            return HotspotDetection {
                detected: true,
                ip: Some(ip.clone()),
                network_type: Some("Android Hotspot".to_string()),
            };
        }
        if ip.starts_with("172.20.10.") {
            return HotspotDetection {
                detected: true,
                ip: Some(ip.clone()),
                network_type: Some("iPhone/iOS Hotspot".to_string()),
            };
        }
        if ip.starts_with("192.168.137.") {
            return HotspotDetection {
                detected: true,
                ip: Some(ip.clone()),
                network_type: Some("Windows Mobile Hotspot".to_string()),
            };
        }
        if ip.starts_with("192.168.49.") {
            return HotspotDetection {
                detected: true,
                ip: Some(ip.clone()),
                network_type: Some("Android Wi-Fi Direct".to_string()),
            };
        }
    }

    HotspotDetection {
        detected: false,
        ip: None,
        network_type: None,
    }
}

/// Returns true if the given IP is in a known phone hotspot range.
pub fn is_phone_hotspot_ip(ip: &str) -> bool {
    PHONE_HOTSPOT_PREFIXES.iter().any(|prefix| ip.starts_with(prefix))
}

/// Get all local IPv4 addresses (simple cross-platform scan).
fn get_all_local_ips() -> Vec<String> {
    let mut ips = Vec::new();

    // Try connecting to various local ranges to discover our IPs
    let targets = [
        "192.168.43.1:80",  // Android hotspot gateway
        "172.20.10.1:80",   // iOS hotspot gateway
        "192.168.137.1:80", // Windows hotspot gateway
        "192.168.49.1:80",  // Android Wi-Fi Direct
        "192.168.1.1:80",   // Common router
        "192.168.0.1:80",   // Common router
        "10.0.0.1:80",      // Common router
    ];

    for target in &targets {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if socket.connect(target).is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    let ip = addr.ip().to_string();
                    if ip != "127.0.0.1" && !ips.contains(&ip) {
                        ips.push(ip);
                    }
                }
            }
        }
    }

    ips
}

// ─── Hotspot Instructions (Phone-First) ──────────────────────────────────────

/// Returns phone-first instructions for offline file transfer.
///
/// The primary approach is to use the PHONE as the hotspot (works on
/// every phone, even without cellular data, in airplane mode, etc.)
/// The laptop just connects to the phone's hotspot like any Wi-Fi.
pub fn hotspot_instructions_phone() -> Vec<String> {
    vec![
        "══ PHONE-AS-HOTSPOT (Recommended — Works Anywhere) ══".into(),
        "".into(),
        "1. Turn on Hotspot on your Phone:".into(),
        "   • Android: Swipe down from the top to open Quick Settings and tap 'Hotspot'.".into(),
        "   • iPhone: Open Settings → Personal Hotspot → toggle 'Allow Others to Join' ON.".into(),
        "   • Alternative (Universal): Open Settings, search for 'hotspot' in the search bar.".into(),
        "   • Note down the hotspot Wi-Fi Name (SSID) and Password shown on the screen.".into(),
        "".into(),
        "2. Connect your Laptop:".into(),
        "   • Click the Wi-Fi icon in your laptop taskbar/menu bar.".into(),
        "   • Select your phone's Wi-Fi network and enter the password to connect.".into(),
        "".into(),
        "3. Start FileDrop:".into(),
        "   • FileDrop will auto-prioritize the connection and display a QR code.".into(),
        "   • Scan the QR code with your phone to open the file sharing page.".into(),
    ]
}

/// Returns platform-specific instructions for creating a hotspot FROM the laptop.
/// This is the secondary/advanced option.
pub fn hotspot_instructions_laptop(os: &str) -> Vec<String> {
    match os {
        "windows" => vec![
            "══ LAPTOP-AS-HOTSPOT (Advanced — Windows) ══".into(),
            "".into(),
            "Option 1 — Settings GUI:".into(),
            "  1. Open Settings (Win + I)".into(),
            "  2. Go to Network & Internet → Mobile Hotspot".into(),
            "  3. Set the network name and password".into(),
            "  4. Toggle \"Mobile Hotspot\" to On".into(),
            "  5. Connect your phone to the hotspot network".into(),
            "".into(),
            "⚠ Note: Windows may require an active internet".into(),
            "  connection to enable Mobile Hotspot.".into(),
            "  If so, use the Phone-as-Hotspot method instead.".into(),
        ],
        "macos" => vec![
            "══ LAPTOP-AS-HOTSPOT (Advanced — macOS) ══".into(),
            "".into(),
            "  1. Open System Settings → General → Sharing".into(),
            "  2. Click \"Internet Sharing\"".into(),
            "  3. Share from: any connection (or none)".into(),
            "  4. To devices using: Wi-Fi".into(),
            "  5. Click \"Wi-Fi Options\" → set name & password".into(),
            "  6. Toggle Internet Sharing ON".into(),
            "  7. Connect your phone to that network".into(),
        ],
        "linux" => vec![
            "══ LAPTOP-AS-HOTSPOT (Advanced — Linux) ══".into(),
            "".into(),
            format!(
                "  nmcli device wifi hotspot ssid {} ifname {} password {}",
                DEFAULT_SSID, DEFAULT_WIFI_IFACE, DEFAULT_PASSWORD
            ),
            "".into(),
            "  Or let FileDrop create it automatically (press [A]).".into(),
            "".into(),
            format!(
                "  Connect your phone to \"{}\" with password \"{}\".",
                DEFAULT_SSID, DEFAULT_PASSWORD
            ),
        ],
        other => vec![
            format!("══ LAPTOP-AS-HOTSPOT ({}) ══", other),
            "".into(),
            "  Automatic setup not available on this platform.".into(),
            "  Please create a hotspot manually or use Phone-as-Hotspot.".into(),
        ],
    }
}

/// Legacy compatibility: Returns combined instructions for the given OS.
pub fn hotspot_instructions(os: &str) -> Vec<String> {
    let mut all = hotspot_instructions_phone();
    all.push("".into());
    all.extend(hotspot_instructions_laptop(os));
    all
}

// ─── Auto-Create Hotspot ─────────────────────────────────────────────────────

/// Automatically creates a Wi-Fi hotspot on Linux using `nmcli`.
pub async fn auto_create_hotspot() -> Result<(String, String)> {
    let os = detect_os();
    if os == "windows" {
        return auto_create_hotspot_windows().await;
    }
    if os != "linux" {
        anyhow::bail!(
            "[HOTSPOT] Auto-create is only supported on Linux and Windows (current OS: {}). \
             Please create a hotspot manually or use your phone as a hotspot.",
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

/// Attempts to create a Wi-Fi hotspot on Windows using netsh (best-effort).
///
/// Note: This uses the deprecated `netsh wlan set hostednetwork` command.
/// It may not work on all modern Wi-Fi adapters (especially on Windows 11).
/// The phone-as-hotspot approach is more reliable.
async fn auto_create_hotspot_windows() -> Result<(String, String)> {
    tracing::info!(
        "[HOTSPOT] Attempting Windows hotspot: ssid={} password={}",
        DEFAULT_SSID,
        DEFAULT_PASSWORD
    );

    // Step 1: Configure the hosted network
    let configure = tokio::process::Command::new("netsh")
        .args([
            "wlan",
            "set",
            "hostednetwork",
            "mode=allow",
            &format!("ssid={}", DEFAULT_SSID),
            &format!("key={}", DEFAULT_PASSWORD),
        ])
        .output()
        .await
        .context("[HOTSPOT] Failed to execute netsh — run as Administrator")?;

    if !configure.status.success() {
        let stderr = String::from_utf8_lossy(&configure.stderr);
        let stdout = String::from_utf8_lossy(&configure.stdout);
        anyhow::bail!(
            "[HOTSPOT] netsh configuration failed. Your Wi-Fi adapter may not support hosted networks.\n\
             Try using your phone as a hotspot instead.\n\
             stdout: {}\nstderr: {}",
            stdout.trim(),
            stderr.trim()
        );
    }

    // Step 2: Start the hosted network
    let start = tokio::process::Command::new("netsh")
        .args(["wlan", "start", "hostednetwork"])
        .output()
        .await
        .context("[HOTSPOT] Failed to start hosted network")?;

    if !start.status.success() {
        let stderr = String::from_utf8_lossy(&start.stderr);
        let stdout = String::from_utf8_lossy(&start.stdout);
        anyhow::bail!(
            "[HOTSPOT] Failed to start hosted network.\n\
             This often happens without an internet connection on Windows.\n\
             Use your PHONE as a hotspot instead (more reliable).\n\
             stdout: {}\nstderr: {}",
            stdout.trim(),
            stderr.trim()
        );
    }

    tracing::info!("[HOTSPOT] Windows hosted network started successfully");
    Ok((DEFAULT_SSID.to_string(), DEFAULT_PASSWORD.to_string()))
}

// ─── Cleanup Instructions ────────────────────────────────────────────────────

/// Returns an OS-specific command to tear down a previously created hotspot.
pub fn cleanup_instructions(os: &str) -> Option<String> {
    match os {
        "linux" => Some(format!("nmcli connection delete {}", DEFAULT_SSID)),
        "windows" => Some("netsh wlan stop hostednetwork".to_string()),
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
        assert_eq!(ConnectionMode::from_str("offline"), Some(ConnectionMode::Hotspot));
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
        assert!(!os.is_empty(), "OS string should not be empty");
    }

    #[test]
    fn test_phone_hotspot_detection_ranges() {
        assert!(is_phone_hotspot_ip("192.168.43.5"));
        assert!(is_phone_hotspot_ip("172.20.10.3"));
        assert!(is_phone_hotspot_ip("192.168.137.1"));
        assert!(is_phone_hotspot_ip("192.168.49.2"));
        assert!(!is_phone_hotspot_ip("192.168.1.100"));
        assert!(!is_phone_hotspot_ip("10.0.0.5"));
    }

    #[test]
    fn test_hotspot_instructions_phone() {
        let instructions = hotspot_instructions_phone();
        assert!(!instructions.is_empty());
        let joined = instructions.join("\n");
        assert!(joined.contains("Android"));
        assert!(joined.contains("iPhone"));
        assert!(joined.contains("filedrop receive"));
    }

    #[test]
    fn test_hotspot_instructions_laptop_windows() {
        let instructions = hotspot_instructions_laptop("windows");
        assert!(!instructions.is_empty());
        let joined = instructions.join("\n");
        assert!(joined.contains("Windows"));
        assert!(joined.contains("Mobile Hotspot"));
    }

    #[test]
    fn test_hotspot_instructions_laptop_macos() {
        let instructions = hotspot_instructions_laptop("macos");
        assert!(!instructions.is_empty());
        let joined = instructions.join("\n");
        assert!(joined.contains("macOS"));
        assert!(joined.contains("Internet Sharing"));
    }

    #[test]
    fn test_hotspot_instructions_laptop_linux() {
        let instructions = hotspot_instructions_laptop("linux");
        assert!(!instructions.is_empty());
        let joined = instructions.join("\n");
        assert!(joined.contains("nmcli"));
        assert!(joined.contains(DEFAULT_SSID));
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
    fn test_cleanup_instructions_windows() {
        let cmd = cleanup_instructions("windows");
        assert!(cmd.is_some());
        assert!(cmd.unwrap().contains("netsh"));
    }

    #[test]
    fn test_cleanup_instructions_macos() {
        assert!(cleanup_instructions("macos").is_none());
    }

    #[tokio::test]
    async fn test_auto_create_hotspot_non_linux_non_windows() {
        if detect_os() != "linux" && detect_os() != "windows" {
            let result = auto_create_hotspot().await;
            assert!(result.is_err());
        }
    }
}

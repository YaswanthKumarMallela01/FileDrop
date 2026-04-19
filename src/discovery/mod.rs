//! mDNS service discovery using the mdns-sd crate.
//!
//! Advertises the laptop as "_filedrop._tcp.local" so paired phones
//! can discover it automatically on the LAN.
//! Also supports browsing for FileDrop peers.

use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceInfo, ServiceEvent};
use std::collections::HashMap;
use std::net::IpAddr;
use thiserror::Error;

/// The mDNS service type for FileDrop
const SERVICE_TYPE: &str = "_filedrop._tcp.local.";

/// Discovery-related errors
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum DiscoveryError {
    #[error("mDNS daemon failed to start: {0}")]
    DaemonFailed(String),

    #[error("Service registration failed: {0}")]
    RegistrationFailed(String),

    #[error("No peers found within timeout")]
    NoPeersFound,
}

/// A discovered FileDrop peer on the network
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DiscoveredPeer {
    /// Peer's display name
    pub name: String,
    /// Peer's IP address
    pub addr: IpAddr,
    /// Peer's port
    pub port: u16,
    /// Certificate fingerprint (from TXT record)
    pub fingerprint: Option<String>,
}

/// Start advertising this device as a FileDrop service on mDNS.
/// Returns the daemon handle which must be kept alive.
pub fn advertise_service(
    device_name: &str,
    port: u16,
    fingerprint: &str,
) -> Result<ServiceDaemon> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| DiscoveryError::DaemonFailed(e.to_string()))?;

    // Build properties for TXT record
    let mut properties = HashMap::new();
    properties.insert("fingerprint".to_string(), fingerprint.to_string());
    properties.insert("version".to_string(), "0.1.0".to_string());

    let host_name = format!("{}.local.", device_name.to_lowercase().replace(' ', "-"));

    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        device_name,
        &host_name,
        "",
        port,
        properties,
    )
    .map_err(|e| DiscoveryError::RegistrationFailed(e.to_string()))?;

    mdns.register(service_info)
        .map_err(|e| DiscoveryError::RegistrationFailed(e.to_string()))?;

    tracing::info!(
        "mDNS: Advertising '{}' on {}:{}",
        device_name,
        SERVICE_TYPE,
        port
    );

    Ok(mdns)
}

/// Browse for FileDrop peers on the network.
/// Returns discovered peers within the given timeout.
pub async fn browse_peers(timeout_secs: u64) -> Result<Vec<DiscoveredPeer>> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| DiscoveryError::DaemonFailed(e.to_string()))?;

    let receiver = mdns
        .browse(SERVICE_TYPE)
        .map_err(|e| DiscoveryError::DaemonFailed(e.to_string()))?;

    let mut peers = Vec::new();
    let deadline = tokio::time::Instant::now()
        + tokio::time::Duration::from_secs(timeout_secs);

    tracing::info!("mDNS: Browsing for FileDrop peers ({} seconds)...", timeout_secs);

    loop {
        let remaining = deadline.duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, tokio::task::spawn_blocking({
            let receiver = receiver.clone();
            move || receiver.recv()
        }))
        .await
        {
            Ok(Ok(Ok(event))) => {
                if let ServiceEvent::ServiceResolved(info) = event {
                    let name = info.get_fullname().to_string();
                    let port = info.get_port();
                    let fingerprint = info
                        .get_properties()
                        .get("fingerprint")
                        .map(|v| v.val_str().to_string());

                    for addr in info.get_addresses() {
                        tracing::info!("mDNS: Found peer '{}' at {}:{}", name, addr, port);
                        peers.push(DiscoveredPeer {
                            name: name.clone(),
                            addr: *addr,
                            port,
                            fingerprint: fingerprint.clone(),
                        });
                    }
                }
            }
            Ok(Ok(Err(e))) => {
                tracing::debug!("mDNS browse recv error: {}", e);
                break;
            }
            Ok(Err(e)) => {
                tracing::debug!("mDNS browse task error: {}", e);
                break;
            }
            Err(_) => {
                // Timeout
                break;
            }
        }
    }

    // Shutdown mdns daemon
    let _ = mdns.shutdown();

    if peers.is_empty() {
        tracing::warn!("No FileDrop peers found on the network");
    }

    Ok(peers)
}

/// Stop advertising a service
#[allow(dead_code)]
pub fn stop_advertising(mdns: ServiceDaemon) -> Result<()> {
    mdns.shutdown().map_err(|e| {
        anyhow::anyhow!("Failed to shutdown mDNS daemon: {}", e)
    })?;

    tracing::info!("mDNS: Stopped advertising");
    Ok(())
}

//! Certificate generation, loading, and peer management.
//!
//! Uses `rcgen` for self-signed certificate generation at first run.
//! Certificates are stored in `~/.config/filedrop/certs/`.
//! Peer certificates are stored in `~/.config/filedrop/peers/`.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

use crate::config;

/// Certificate-related errors
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum CertError {
    #[error("Certificate not found at {path}")]
    NotFound { path: String },

    #[error("Invalid certificate: {reason}")]
    Invalid { reason: String },

    #[error("Peer not found: {name}")]
    PeerNotFound { name: String },

    #[error("Certificate generation failed: {0}")]
    GenerationFailed(String),
}

/// Holds loaded certificate and key data
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CertificateBundle {
    /// PEM-encoded certificate
    pub cert_pem: String,
    /// PEM-encoded private key
    pub key_pem: String,
    /// Path to the certificate file
    pub cert_path: PathBuf,
    /// Path to the key file
    pub key_path: PathBuf,
    /// SHA-256 fingerprint of the certificate
    pub fingerprint: String,
}

/// Information about a paired peer
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PeerInfo {
    /// Peer display name
    pub name: String,
    /// SHA-256 fingerprint of peer's certificate
    pub fingerprint: String,
    /// When the peer was paired
    pub paired_at: String,
    /// Path to the peer's certificate file
    pub cert_path: PathBuf,
}

/// Ensure certificates exist, generating them if needed on first run
pub fn ensure_certificates() -> Result<CertificateBundle> {
    let certs_dir = config::certs_dir()?;
    fs::create_dir_all(&certs_dir)?;

    let cert_path = certs_dir.join("filedrop.crt");
    let key_path = certs_dir.join("filedrop.key");

    if cert_path.exists() && key_path.exists() {
        tracing::info!("Loading existing certificates");
        load_certificates(&cert_path, &key_path)
    } else {
        tracing::info!("Generating new self-signed certificates");
        generate_certificates(&cert_path, &key_path)
    }
}

/// Generate a new self-signed certificate using rcgen
fn generate_certificates(cert_path: &PathBuf, key_path: &PathBuf) -> Result<CertificateBundle> {
    use rcgen::{CertificateParams, KeyPair};

    // Build certificate parameters
    let mut params = CertificateParams::new(vec!["filedrop.local".to_string()])
        .map_err(|e| CertError::GenerationFailed(e.to_string()))?;

    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "FileDrop Device");
    params
        .distinguished_name
        .push(rcgen::DnType::OrganizationName, "FileDrop");

    // Certificate valid for 10 years
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2034, 1, 1);

    // Add SAN for IP addresses on local network
    params
        .subject_alt_names
        .push(rcgen::SanType::DnsName("filedrop.local".try_into().unwrap()));

    // Generate key pair
    let key_pair = KeyPair::generate().map_err(|e| CertError::GenerationFailed(e.to_string()))?;

    // Self-sign the certificate
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| CertError::GenerationFailed(e.to_string()))?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // Save to disk
    fs::write(cert_path, &cert_pem)
        .with_context(|| format!("Failed to write certificate to {}", cert_path.display()))?;
    fs::write(key_path, &key_pem)
        .with_context(|| format!("Failed to write key to {}", key_path.display()))?;

    // Calculate fingerprint
    let fingerprint = calculate_fingerprint(&cert_pem);

    tracing::info!("Certificate generated with fingerprint: {}", fingerprint);

    Ok(CertificateBundle {
        cert_pem,
        key_pem,
        cert_path: cert_path.clone(),
        key_path: key_path.clone(),
        fingerprint,
    })
}

/// Load existing certificates from disk
fn load_certificates(cert_path: &PathBuf, key_path: &PathBuf) -> Result<CertificateBundle> {
    let cert_pem = fs::read_to_string(cert_path)
        .with_context(|| format!("Failed to read certificate from {}", cert_path.display()))?;
    let key_pem = fs::read_to_string(key_path)
        .with_context(|| format!("Failed to read key from {}", key_path.display()))?;

    let fingerprint = calculate_fingerprint(&cert_pem);

    Ok(CertificateBundle {
        cert_pem,
        key_pem,
        cert_path: cert_path.clone(),
        key_path: key_path.clone(),
        fingerprint,
    })
}

/// Calculate SHA-256 fingerprint of a PEM certificate
fn calculate_fingerprint(cert_pem: &str) -> String {
    use sha2::{Digest, Sha256};

    // Extract DER bytes from PEM
    let pem_lines: Vec<&str> = cert_pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let der_b64 = pem_lines.join("");

    if let Ok(der_bytes) = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &der_b64,
    ) {
        let hash = Sha256::digest(&der_bytes);
        hex::encode(hash)
            .chars()
            .collect::<Vec<_>>()
            .chunks(2)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(":")
            .to_uppercase()
    } else {
        "UNKNOWN".to_string()
    }
}

/// Save a peer's certificate after pairing
pub fn save_peer(name: &str, cert_pem: &str) -> Result<()> {
    let peers_dir = config::peers_dir()?;
    fs::create_dir_all(&peers_dir)?;

    let peer_path = peers_dir.join(format!("{}.pem", name));
    fs::write(&peer_path, cert_pem)
        .with_context(|| format!("Failed to save peer certificate for {}", name))?;

    // Save metadata
    let meta_path = peers_dir.join(format!("{}.meta", name));
    let metadata = format!(
        "name={}\npaired_at={}\n",
        name,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    fs::write(&meta_path, metadata)?;

    tracing::info!("Peer '{}' saved to {}", name, peer_path.display());
    Ok(())
}

/// List all paired peers
pub fn list_peers() -> Result<Vec<PeerInfo>> {
    let peers_dir = config::peers_dir()?;

    if !peers_dir.exists() {
        return Ok(Vec::new());
    }

    let mut peers = Vec::new();

    for entry in fs::read_dir(&peers_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("pem") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let cert_pem = fs::read_to_string(&path)?;
            let fingerprint = calculate_fingerprint(&cert_pem);

            // Try to read metadata
            let meta_path = peers_dir.join(format!("{}.meta", name));
            let paired_at = if meta_path.exists() {
                let meta = fs::read_to_string(&meta_path)?;
                meta.lines()
                    .find(|l| l.starts_with("paired_at="))
                    .map(|l| l.trim_start_matches("paired_at=").to_string())
                    .unwrap_or_else(|| "Unknown".to_string())
            } else {
                "Unknown".to_string()
            };

            peers.push(PeerInfo {
                name,
                fingerprint,
                paired_at,
                cert_path: path,
            });
        }
    }

    Ok(peers)
}

/// Remove a paired peer by name
pub fn remove_peer(name: &str) -> Result<()> {
    let peers_dir = config::peers_dir()?;
    let peer_path = peers_dir.join(format!("{}.pem", name));
    let meta_path = peers_dir.join(format!("{}.meta", name));

    if !peer_path.exists() {
        return Err(CertError::PeerNotFound {
            name: name.to_string(),
        }
        .into());
    }

    fs::remove_file(&peer_path)
        .with_context(|| format!("Failed to remove peer certificate for {}", name))?;

    if meta_path.exists() {
        fs::remove_file(&meta_path).ok();
    }

    tracing::info!("Peer '{}' removed", name);
    Ok(())
}

/// Load a peer's certificate by name
#[allow(dead_code)]
pub fn load_peer_cert(name: &str) -> Result<String> {
    let peers_dir = config::peers_dir()?;
    let peer_path = peers_dir.join(format!("{}.pem", name));

    if !peer_path.exists() {
        return Err(CertError::PeerNotFound {
            name: name.to_string(),
        }
        .into());
    }

    Ok(fs::read_to_string(&peer_path)?)
}

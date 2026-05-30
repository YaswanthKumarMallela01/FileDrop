//! Standalone Axum HTTP server for ephemeral file sharing.
//!
//! `filedrop share <file> [options]` spins up a temporary HTTP server on port 7879
//! that serves a single file (or zipped folder) behind a cryptographically random
//! token URL. Features include:
//!
//! - Automatic expiry after a configurable duration (default 15 minutes)
//! - One-time download mode (`--once`)
//! - Optional 4-digit PIN protection
//! - QR code displayed in terminal for easy phone scanning
//! - SHA-256 hash verification displayed in terminal summary
//! - Hacker-themed HTML download page (matrix green on black)
//! - Graceful shutdown on Ctrl+C or expiry

use anyhow::{Context, Result};
use axum::{
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::Notify;

/// Default share server port (separate from the main transfer port 7878).
const SHARE_PORT: u16 = 7879;

/// Chunk size for streaming file content to HTTP clients.
const STREAM_CHUNK_SIZE: usize = 256 * 1024; // 256 KB

/// Cookie name used for PIN-authenticated sessions.
const PIN_COOKIE_NAME: &str = "filedrop_pin_auth";

/// Cookie value set after successful PIN verification.
const PIN_COOKIE_VALUE: &str = "verified";

// ─── Shared Application State ───────────────────────────────────────────────

/// Shared state for all Axum route handlers.
#[derive(Debug)]
struct ShareState {
    /// The random token embedded in the URL path.
    token: String,
    /// Canonical path to the file being shared (or temp zip for folders).
    file_path: PathBuf,
    /// Display name shown to the downloader.
    file_name: String,
    /// File size in bytes.
    file_size: u64,
    /// Pre-computed SHA-256 hex digest of the file.
    sha256: String,
    /// Whether the link expires after the first successful download.
    once: bool,
    /// Optional 4-digit PIN required before download.
    pin: Option<String>,
    /// Notifier that triggers graceful server shutdown.
    shutdown: Arc<Notify>,
    /// Display name of the laptop creating the share
    device_name: String,
}

// ─── Public Entry Point ─────────────────────────────────────────────────────

/// Start the ephemeral share server.
///
/// This is the main entry point called by the `filedrop share` CLI subcommand.
/// It computes the file hash, generates a random token, prints a terminal summary
/// with QR code, and runs the Axum server until expiry / one-time download / Ctrl+C.
///
/// # Arguments
///
/// * `path`      — File or directory to share.
/// * `expires`   — How long the link stays alive (e.g. parsed from `"10m"`).
/// * `once`      — If `true`, expire immediately after the first download.
/// * `pin`       — Optional 4-digit PIN the downloader must enter.
/// * `local_ips` — LAN IP addresses detected on this machine.
pub async fn run_share(
    selected_paths: Vec<PathBuf>,
    expires: Duration,
    once: bool,
    pin: Option<String>,
    local_ips: &[String],
    hotspot_mode: bool,
    device_name: &str,
) -> Result<()> {
    if selected_paths.is_empty() {
        anyhow::bail!("No files or folders selected to share");
    }

    let (file_path, file_name, is_temp_zip) = if selected_paths.len() == 1 {
        let path = tokio::fs::canonicalize(&selected_paths[0])
            .await
            .with_context(|| format!("Path does not exist: {}", selected_paths[0].display()))?;

        if path.is_dir() {
            // Zip the folder into a temporary file for streaming
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("folder")
                .to_string();

            let zip_name = format!("{}.zip", dir_name);
            let zip_path = std::env::temp_dir().join(&zip_name);
            zip_directory(&path, &zip_path).await?;
            (zip_path, zip_name, true)
        } else {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            (path.clone(), name, false)
        }
    } else {
        // Zip all selected files into a temporary zip file
        let zip_name = format!("{}_filedrop_shared.zip", device_name.replace(' ', "_"));
        let zip_path = std::env::temp_dir().join(&zip_name);
        zip_files(&selected_paths, &zip_path).await?;
        (zip_path, zip_name, true)
    };

    // ── Compute SHA-256 ──────────────────────────────────────────────────
    let sha256 = compute_sha256(&file_path).await?;

    // ── File size ────────────────────────────────────────────────────────
    let metadata = tokio::fs::metadata(&file_path)
        .await
        .context("Failed to read file metadata")?;
    let file_size = metadata.len();

    // ── Generate token ───────────────────────────────────────────────────
    let token = generate_token(8);

    // ── Build the public URL ─────────────────────────────────────────────
    let primary_ip = local_ips
        .first()
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let share_url = format!("http://{}:{}/get/{}", primary_ip, SHARE_PORT, token);

    // ── Print terminal summary ───────────────────────────────────────────
    if hotspot_mode {
        println!();
        println!("  \x1b[33m[HOTSPOT MODE] Connect your phone to the hotspot Wi-Fi first, then scan this QR\x1b[0m");
    }
    print_terminal_summary(
        &share_url,
        &file_name,
        file_size,
        &sha256,
        &expires,
        once,
        pin.as_deref(),
    );

    // ── Build shared state ───────────────────────────────────────────────
    let shutdown = Arc::new(Notify::new());
    let state = Arc::new(ShareState {
        token: token.clone(),
        file_path: file_path.clone(),
        file_name,
        file_size,
        sha256,
        once,
        pin,
        shutdown: Arc::clone(&shutdown),
        device_name: device_name.to_string(),
    });

    // ── Build Axum router ────────────────────────────────────────────────
    let app = Router::new()
        .route("/get/{token}", get(handle_landing))
        .route("/get/{token}/download", get(handle_download))
        .route("/get/{token}/verify-pin", post(handle_verify_pin))
        .with_state(state);

    // ── Bind listener ────────────────────────────────────────────────────
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], SHARE_PORT));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind share server on port {}", SHARE_PORT))?;

    println!("[SHARE] Server listening on http://0.0.0.0:{}", SHARE_PORT);

    // ── Spawn expiry timer ───────────────────────────────────────────────
    let shutdown_expiry = Arc::clone(&shutdown);
    tokio::spawn(async move {
        tokio::time::sleep(expires).await;
        println!("\n[SHARE] Link expired. Session closed.");
        shutdown_expiry.notify_waiters();
    });

    // ── Spawn Ctrl+C handler ─────────────────────────────────────────────
    let shutdown_ctrlc = Arc::clone(&shutdown);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            println!("\n[SHARE] Interrupted. Session closed.");
            shutdown_ctrlc.notify_waiters();
        }
    });

    // ── Run server until shutdown ────────────────────────────────────────
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.notified().await;
        })
        .await
        .context("Share server error")?;

    // ── Cleanup temp zip if we created one ───────────────────────────────
    if is_temp_zip {
        let _ = tokio::fs::remove_file(&file_path).await;
    }

    Ok(())
}

// ─── Duration Parsing ───────────────────────────────────────────────────────

/// Parse a human-friendly duration string into a [`Duration`].
///
/// Supported suffixes:
/// - `s` — seconds  (e.g. `"30s"`)
/// - `m` — minutes  (e.g. `"15m"`)
/// - `h` — hours    (e.g. `"2h"`)
///
/// # Errors
///
/// Returns an error if the string has no numeric part or an unrecognised suffix.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// let d = parse_duration("10m").unwrap();
/// assert_eq!(d, Duration::from_secs(600));
/// ```
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if s.is_empty() {
        anyhow::bail!("Empty duration string");
    }

    let (num_part, suffix) = s.split_at(
        s.find(|c: char| !c.is_ascii_digit())
            .unwrap_or(s.len()),
    );

    let value: u64 = num_part
        .parse()
        .with_context(|| format!("Invalid number in duration: '{}'", s))?;

    let seconds = match suffix.trim() {
        "s" | "" => value,
        "m" => value * 60,
        "h" => value * 3600,
        other => anyhow::bail!("Unknown duration suffix '{}' in '{}'", other, s),
    };

    Ok(Duration::from_secs(seconds))
}

// ─── Route Handlers ─────────────────────────────────────────────────────────

/// `GET /get/:token` — Landing page.
///
/// If a PIN is configured and the client has not yet verified, renders the PIN
/// entry form. Otherwise, renders the download page with file metadata.
async fn handle_landing(
    AxumPath(token): AxumPath<String>,
    State(state): State<Arc<ShareState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    if token != state.token {
        return (StatusCode::NOT_FOUND, Html(html_error("Link not found or expired.")))
            .into_response();
    }

    // PIN gate: check cookie
    if state.pin.is_some() && !is_pin_verified(&headers) {
        return Html(html_pin_form(&token)).into_response();
    }

    Html(html_download_page(
        &state.file_name,
        state.file_size,
        &state.sha256,
        &token,
        &state.device_name,
    ))
    .into_response()
}

/// `GET /get/:token/download` — Stream the file as an attachment.
///
/// Sends the file with `Content-Disposition: attachment` so the browser
/// triggers a download dialog. If `--once` was set, signals shutdown after
/// the response body has been fully sent.
async fn handle_download(
    AxumPath(token): AxumPath<String>,
    State(state): State<Arc<ShareState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    if token != state.token {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    // PIN gate
    if state.pin.is_some() && !is_pin_verified(&headers) {
        return (StatusCode::FORBIDDEN, "PIN required").into_response();
    }

    // Open the file for streaming
    let file = match tokio::fs::File::open(&state.file_path).await {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Cannot read file: {}", e),
            )
                .into_response();
        }
    };

    // Build a streaming body using axum::body::Body::from_stream
    let stream = tokio_util::io::ReaderStream::with_capacity(file, STREAM_CHUNK_SIZE);
    let body = axum::body::Body::from_stream(stream);

    // If --once, schedule shutdown after a short delay to let the response flush
    if state.once {
        let shutdown = Arc::clone(&state.shutdown);
        tokio::spawn(async move {
            // Give the HTTP response a moment to complete
            tokio::time::sleep(Duration::from_secs(2)).await;
            println!("\n[SHARE] Link expired. Session closed.");
            shutdown.notify_waiters();
        });
    }

    let content_disposition = format!("attachment; filename=\"{}\"", state.file_name);

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_DISPOSITION, content_disposition),
            (header::CONTENT_LENGTH, state.file_size.to_string()),
        ],
        body,
    )
        .into_response()
}

/// `POST /get/:token/verify-pin` — Validate the submitted PIN.
///
/// On success, sets a session cookie and redirects back to the landing page.
/// On failure, renders the PIN form with an error message.
async fn handle_verify_pin(
    AxumPath(token): AxumPath<String>,
    State(state): State<Arc<ShareState>>,
    axum::Form(form): axum::Form<PinForm>,
) -> Response {
    if token != state.token {
        return (StatusCode::NOT_FOUND, Html(html_error("Link not found or expired.")))
            .into_response();
    }

    let expected = match &state.pin {
        Some(p) => p,
        None => {
            // No PIN required — just redirect
            return redirect_to_landing(&token);
        }
    };

    if form.pin.trim() == expected {
        // Set cookie and redirect
        let cookie = format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=86400",
            PIN_COOKIE_NAME, PIN_COOKIE_VALUE
        );
        let mut resp = redirect_to_landing(&token);
        resp.headers_mut().insert(
            header::SET_COOKIE,
            cookie.parse().unwrap(),
        );
        resp
    } else {
        Html(html_pin_form_with_error(&token, "Incorrect PIN. Try again.")).into_response()
    }
}

/// Simple form data for PIN submission.
#[derive(serde::Deserialize)]
struct PinForm {
    pin: String,
}

// ─── Cookie Helpers ─────────────────────────────────────────────────────────

/// Check whether the request carries a valid PIN-auth cookie.
fn is_pin_verified(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|cookies| {
            cookies.split(';').any(|c| {
                let c = c.trim();
                c == format!("{}={}", PIN_COOKIE_NAME, PIN_COOKIE_VALUE)
            })
        })
        .unwrap_or(false)
}

/// Build a 303 See Other redirect to the landing page.
fn redirect_to_landing(token: &str) -> Response {
    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, format!("/get/{}", token))],
        "",
    )
        .into_response()
}

// ─── Token Generation ───────────────────────────────────────────────────────

/// Generate a cryptographically random alphanumeric token.
fn generate_token(len: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

// ─── SHA-256 ────────────────────────────────────────────────────────────────

/// Compute the SHA-256 hex digest of a file.
async fn compute_sha256(path: &PathBuf) -> Result<String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("Cannot open file for hashing: {}", path.display()))?;

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; STREAM_CHUNK_SIZE];

    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

// ─── Folder Zipping ─────────────────────────────────────────────────────────

/// Create a zip archive from a directory.
///
/// Uses the `zip` crate (already in Cargo.toml) to build a standard ZIP file
/// from the given directory, walking it recursively. The resulting archive is
/// written to `output`.
async fn zip_directory(dir: &std::path::Path, output: &std::path::Path) -> Result<()> {
    let dir = dir.to_path_buf();
    let output = output.to_path_buf();

    // Zip I/O is blocking, so run in spawn_blocking
    tokio::task::spawn_blocking(move || {
        use std::fs;
        use std::io::{Read, Write};
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let file = fs::File::create(&output)
            .with_context(|| format!("Cannot create zip: {}", output.display()))?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let base = &dir;
        let mut stack = vec![base.to_path_buf()];

        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current)? {
                let entry = entry?;
                let path = entry.path();
                let rel = path.strip_prefix(base).unwrap_or(&path);
                let name = rel.to_string_lossy().replace('\\', "/");

                if path.is_dir() {
                    zip.add_directory(&format!("{}/", name), options)?;
                    stack.push(path);
                } else {
                    zip.start_file(&name, options)?;
                    let mut f = fs::File::open(&path)?;
                    let mut buf = vec![0u8; 64 * 1024];
                    loop {
                        let n = f.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        zip.write_all(&buf[..n])?;
                    }
                }
            }
        }

        zip.finish()?;
        Ok(())
    })
    .await
    .context("Zip task panicked")?
}

/// Create a zip archive from a list of files/directories.
async fn zip_files(files: &[PathBuf], output: &std::path::Path) -> Result<()> {
    let files = files.to_vec();
    let output = output.to_path_buf();

    tokio::task::spawn_blocking(move || {
        use std::fs;
        use std::io::{Read, Write};
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let file = fs::File::create(&output)
            .with_context(|| format!("Cannot create zip: {}", output.display()))?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for path in files {
            if !path.exists() {
                continue;
            }
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            if path.is_dir() {
                // If it is a directory, zip recursively
                let base = path.parent().unwrap_or(&path);
                let mut stack = vec![path.clone()];
                while let Some(current) = stack.pop() {
                    for entry in fs::read_dir(&current)? {
                        let entry = entry?;
                        let p = entry.path();
                        let rel = p.strip_prefix(base).unwrap_or(&p);
                        let entry_name = rel.to_string_lossy().replace('\\', "/");

                        if p.is_dir() {
                            zip.add_directory(&format!("{}/", entry_name), options)?;
                            stack.push(p);
                        } else {
                            zip.start_file(&entry_name, options)?;
                            let mut f = fs::File::open(&p)?;
                            let mut buf = vec![0u8; 64 * 1024];
                            loop {
                                let n = f.read(&mut buf)?;
                                if n == 0 {
                                    break;
                                }
                                zip.write_all(&buf[..n])?;
                            }
                        }
                    }
                }
            } else {
                // Just a file
                zip.start_file(&name, options)?;
                let mut f = fs::File::open(&path)?;
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = f.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    zip.write_all(&buf[..n])?;
                }
            }
        }

        zip.finish()?;
        Ok(())
    })
    .await
    .context("Zip task panicked")?
}

// ─── Terminal Summary & QR Code ─────────────────────────────────────────────

/// Print the hacker-themed terminal summary box and QR code.
fn print_terminal_summary(
    url: &str,
    file_name: &str,
    file_size: u64,
    sha256: &str,
    expires: &Duration,
    once: bool,
    pin: Option<&str>,
) {
    let size_display = format_bytes(file_size);
    let expiry_display = format_duration(expires);

    println!();
    println!("  \x1b[32m╔══════════════════════════════════════════════════════════════╗\x1b[0m");
    println!("  \x1b[32m║\x1b[0m  \x1b[1;32mFileDrop v0.2.0 — EPHEMERAL SHARE\x1b[0m                           \x1b[32m║\x1b[0m");
    println!("  \x1b[32m╠══════════════════════════════════════════════════════════════╣\x1b[0m");
    println!("  \x1b[32m║\x1b[0m  File    : \x1b[1;32m{:<49}\x1b[0m\x1b[32m║\x1b[0m", file_name);
    println!("  \x1b[32m║\x1b[0m  Size    : {:<49}\x1b[32m║\x1b[0m", size_display);
    println!("  \x1b[32m║\x1b[0m  SHA-256 : {:<49}\x1b[32m║\x1b[0m", &sha256[..std::cmp::min(sha256.len(), 49)]);
    println!("  \x1b[32m║\x1b[0m  Expires : {:<49}\x1b[32m║\x1b[0m", expiry_display);

    if once {
        println!("  \x1b[32m║\x1b[0m  Mode    : \x1b[33mOne-time download{:<32}\x1b[0m\x1b[32m║\x1b[0m", "");
    }
    if let Some(p) = pin {
        println!("  \x1b[32m║\x1b[0m  PIN     : \x1b[33m{:<49}\x1b[0m\x1b[32m║\x1b[0m", p);
    }

    println!("  \x1b[32m╠══════════════════════════════════════════════════════════════╣\x1b[0m");
    println!("  \x1b[32m║\x1b[0m  URL: \x1b[4;32m{:<54}\x1b[0m\x1b[32m║\x1b[0m", url);
    println!("  \x1b[32m╚══════════════════════════════════════════════════════════════╝\x1b[0m");

    // Print QR code
    print_qr_code(url);

    println!();
    println!("  \x1b[90mPress Ctrl+C to stop sharing.\x1b[0m");
    println!();
}

/// Render a QR code to the terminal using Unicode block characters.
fn print_qr_code(data: &str) {
    use qrcode::QrCode;

    let code = match QrCode::new(data.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            println!("  \x1b[31m[SHARE] Failed to generate QR code: {}\x1b[0m", e);
            return;
        }
    };

    let string = code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(2, 1)
        .build();

    println!();
    for line in string.lines() {
        println!("  \x1b[32m{}\x1b[0m", line);
    }
}

// ─── HTML Templates ─────────────────────────────────────────────────────────

/// Render the download landing page as styled HTML.
fn html_download_page(file_name: &str, file_size: u64, sha256: &str, token: &str, device_name: &str) -> String {
    let size_display = format_bytes(file_size);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>FileDrop — {file_name}</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    background: #0a0a0a;
    color: #00FF41;
    font-family: 'Courier New', Courier, monospace;
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 100vh;
    padding: 1rem;
  }}
  .card {{
    border: 1px solid #00FF41;
    padding: 2rem;
    max-width: 500px;
    width: 100%;
    text-align: center;
    box-shadow: 0 0 15px rgba(0,255,65,0.2);
  }}
  h1 {{
    font-size: 1.1rem;
    margin-bottom: 1.5rem;
    letter-spacing: 1px;
    text-transform: uppercase;
  }}
  .meta {{
    text-align: left;
    margin-bottom: 1.5rem;
    font-size: 0.85rem;
    line-height: 1.8;
  }}
  .meta span {{
    color: #33ff66;
  }}
  .download-btn {{
    display: inline-block;
    padding: 0.85rem 1.5rem;
    border: 2px solid #00FF41;
    background: #000;
    color: #00FF41;
    font-family: inherit;
    font-size: 0.95rem;
    font-weight: bold;
    letter-spacing: 1px;
    text-decoration: none;
    text-transform: uppercase;
    cursor: pointer;
    transition: box-shadow 0.2s, background 0.2s;
  }}
  .download-btn:hover {{
    box-shadow: 0 0 15px #00FF41, 0 0 30px rgba(0,255,65,0.3);
    background: #001a00;
  }}
  .hash {{
    margin-top: 1.5rem;
    font-size: 0.7rem;
    word-break: break-all;
    color: #555;
  }}
  .footer {{
    margin-top: 2rem;
    font-size: 0.7rem;
    color: #333;
  }}
</style>
</head>
<body>
<div class="card">
  <h1>[FILEDROP] v0.2.0 :: RECEIVE_MODE</h1>
  <div class="meta" style="margin-top: 1.5rem;">
    <div>File: <span>{file_name}</span></div>
    <div>Size: <span>{size_display}</span></div>
  </div>
  <a class="download-btn" href="/get/{token}/download" style="margin-top: 1rem; width: 100%; text-align: center;">▶ RECEIVE FILES FROM {device_name}</a>
  <div class="hash">SHA-256: {sha256}</div>
  <div class="footer">Ephemeral link — expires automatically</div>
</div>
</body>
</html>"#,
        file_name = html_escape(file_name),
        size_display = size_display,
        token = token,
        sha256 = sha256,
        device_name = html_escape(device_name),
    )
}

/// Render the PIN entry form page.
fn html_pin_form(token: &str) -> String {
    html_pin_form_inner(token, None)
}

/// Render the PIN entry form page with an error message.
fn html_pin_form_with_error(token: &str, error: &str) -> String {
    html_pin_form_inner(token, Some(error))
}

/// Inner implementation for PIN form rendering.
fn html_pin_form_inner(token: &str, error: Option<&str>) -> String {
    let error_html = match error {
        Some(msg) => format!(
            r#"<div style="color:#FF0000;margin-bottom:1rem;font-size:0.85rem;">{}</div>"#,
            html_escape(msg)
        ),
        None => String::new(),
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>FileDrop — PIN Required</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    background: #0a0a0a;
    color: #00FF41;
    font-family: 'Courier New', Courier, monospace;
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 100vh;
    padding: 1rem;
  }}
  .card {{
    border: 1px solid #00FF41;
    padding: 2rem;
    max-width: 400px;
    width: 100%;
    text-align: center;
  }}
  h1 {{
    font-size: 1.2rem;
    margin-bottom: 1.5rem;
    letter-spacing: 2px;
  }}
  input[type="text"] {{
    background: #000;
    border: 2px solid #00FF41;
    color: #00FF41;
    font-family: inherit;
    font-size: 1.5rem;
    text-align: center;
    padding: 0.5rem;
    width: 6rem;
    letter-spacing: 0.5rem;
    outline: none;
    margin-bottom: 1rem;
  }}
  input[type="text"]:focus {{
    box-shadow: 0 0 10px rgba(0,255,65,0.4);
  }}
  button {{
    display: inline-block;
    padding: 0.6rem 1.5rem;
    border: 2px solid #00FF41;
    background: #000;
    color: #00FF41;
    font-family: inherit;
    font-size: 0.9rem;
    font-weight: bold;
    text-transform: uppercase;
    cursor: pointer;
    transition: box-shadow 0.2s, background 0.2s;
  }}
  button:hover {{
    box-shadow: 0 0 15px #00FF41, 0 0 30px rgba(0,255,65,0.3);
    background: #001a00;
  }}
</style>
</head>
<body>
<div class="card">
  <h1>🔒 PIN Required</h1>
  {error_html}
  <form method="POST" action="/get/{token}/verify-pin">
    <div style="margin-bottom:1rem;">
      <input type="text" name="pin" maxlength="4" pattern="[0-9]{{4}}" inputmode="numeric"
             placeholder="····" autocomplete="off" autofocus>
    </div>
    <button type="submit">Verify</button>
  </form>
</div>
</body>
</html>"#,
        error_html = error_html,
        token = token,
    )
}

/// Render a simple error page.
fn html_error(message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>FileDrop — Error</title>
<style>
  body {{
    background: #0a0a0a;
    color: #FF0000;
    font-family: 'Courier New', Courier, monospace;
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 100vh;
  }}
  .card {{
    border: 1px solid #FF0000;
    padding: 2rem;
    max-width: 400px;
    text-align: center;
  }}
</style>
</head>
<body>
<div class="card">
  <h1>✗ Error</h1>
  <p style="margin-top:1rem;">{message}</p>
</div>
</body>
</html>"#,
        message = html_escape(message),
    )
}

// ─── Utility Helpers ────────────────────────────────────────────────────────

/// Minimal HTML escaping for user-supplied strings.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Format bytes into a human-readable string (e.g. `"4.2 MB"`).
fn format_bytes(bytes: u64) -> String {
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

/// Format a [`Duration`] into a human-readable string (e.g. `"15m 0s"`).
fn format_duration(d: &Duration) -> String {
    let total = d.as_secs();
    if total >= 3600 {
        let h = total / 3600;
        let m = (total % 3600) / 60;
        format!("{}h {}m", h, m)
    } else if total >= 60 {
        let m = total / 60;
        let s = total % 60;
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", total)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_seconds() {
        let d = parse_duration("30s").unwrap();
        assert_eq!(d, Duration::from_secs(30));
    }

    #[test]
    fn test_parse_duration_minutes() {
        let d = parse_duration("10m").unwrap();
        assert_eq!(d, Duration::from_secs(600));
    }

    #[test]
    fn test_parse_duration_hours() {
        let d = parse_duration("2h").unwrap();
        assert_eq!(d, Duration::from_secs(7200));
    }

    #[test]
    fn test_parse_duration_bare_number() {
        let d = parse_duration("45").unwrap();
        assert_eq!(d, Duration::from_secs(45));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("").is_err());
        assert!(parse_duration("10x").is_err());
    }

    #[test]
    fn test_generate_token_length() {
        let t = generate_token(8);
        assert_eq!(t.len(), 8);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_token_uniqueness() {
        let a = generate_token(8);
        let b = generate_token(8);
        // Statistically should never collide with 62^8 space
        assert_ne!(a, b);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(&Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(&Duration::from_secs(600)), "10m 0s");
        assert_eq!(format_duration(&Duration::from_secs(3661)), "1h 1m");
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<b>hello</b>"), "&lt;b&gt;hello&lt;/b&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn test_is_pin_verified_no_cookie() {
        let headers = axum::http::HeaderMap::new();
        assert!(!is_pin_verified(&headers));
    }
}

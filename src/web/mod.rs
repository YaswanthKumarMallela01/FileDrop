//! Embedded web UI for phone-based file transfer.
//!
//! Serves a mobile-optimized single-page application directly from
//! the Axum server. The HTML/CSS/JS is compiled into the binary
//! using `include_str!()` — no external files needed at runtime.
//!
//! The web UI connects via WebSocket to the same `/ws` endpoint
//! that the Rust client uses, speaking the same wire protocol.

use axum::{
    response::{Html, IntoResponse},
    http::{header, StatusCode},
};

/// The complete web UI compiled into the binary
const INDEX_HTML: &str = include_str!("index.html");

/// Serve the main web UI page
pub async fn serve_index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

/// Serve a favicon (inline SVG — cyan droplet)
pub async fn serve_favicon() -> impl IntoResponse {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
        <defs><linearGradient id="g" x1="0%" y1="0%" x2="100%" y2="100%">
        <stop offset="0%" style="stop-color:#00D4FF"/><stop offset="100%" style="stop-color:#0088CC"/>
        </linearGradient></defs>
        <circle cx="50" cy="50" r="45" fill="url(#g)"/>
        <text x="50" y="62" font-size="40" text-anchor="middle" fill="#111319">📁</text>
    </svg>"##;

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/svg+xml")],
        svg,
    )
}

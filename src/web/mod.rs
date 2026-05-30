//! Embedded web UI for phone-based file transfer.
//!
//! Serves a mobile-optimized single-page application directly from
//! the Axum server. The HTML/CSS/JS is compiled into the binary
//! using `include_str!()` — no external files needed at runtime.
//!
//! The web UI connects via WebSocket to the same `/ws` endpoint
//! that the Rust client uses, speaking the same wire protocol.
//!
//! Also serves PWA assets: manifest.json, service worker, and icons.

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

// ── PWA Assets (Feature 6) ──────────────────────────────────────────────────

/// Serve the PWA manifest.json
pub async fn serve_manifest() -> impl IntoResponse {
    let manifest = r##"{
  "name": "FileDrop",
  "short_name": "FileDrop",
  "description": "Secure local Wi-Fi file transfer",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#000000",
  "theme_color": "#00FF41",
  "icons": [
    {"src": "/icon-192.png", "sizes": "192x192", "type": "image/png"},
    {"src": "/icon-512.png", "sizes": "512x512", "type": "image/png"}
  ]
}"##;

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        manifest,
    )
}

/// Serve the service worker JavaScript
pub async fn serve_sw() -> impl IntoResponse {
    let sw = r#"const CACHE = 'filedrop-v1';
const ASSETS = ['/', '/manifest.json'];

self.addEventListener('install', e => {
  e.waitUntil(caches.open(CACHE).then(c => c.addAll(ASSETS)));
});

self.addEventListener('fetch', e => {
  // Only cache GET requests for the UI shell; never cache WebSocket or file transfers
  if (e.request.method !== 'GET') return;
  if (e.request.url.includes('/ws')) return;
  e.respondWith(
    caches.match(e.request).then(r => r || fetch(e.request))
  );
});"#;

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        sw,
    )
}

/// Serve the 192x192 PWA icon (generated programmatically)
pub async fn serve_icon_192() -> impl IntoResponse {
    let png_data = generate_fd_icon(192);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png")],
        png_data,
    )
}

/// Serve the 512x512 PWA icon (generated programmatically)
pub async fn serve_icon_512() -> impl IntoResponse {
    let png_data = generate_fd_icon(512);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png")],
        png_data,
    )
}

/// Generate a simple "FD" icon with black background and green text.
///
/// Creates a pixel-art style icon with the letters "F" and "D" rendered
/// as simple block shapes in matrix green (#00FF41) on black (#000000).
fn generate_fd_icon(size: u32) -> Vec<u8> {
    use image::{ImageBuffer, Rgb, ImageEncoder};

    let mut img = ImageBuffer::from_pixel(size, size, Rgb([0u8, 0u8, 0u8]));
    let green = Rgb([0u8, 255u8, 65u8]);

    // Scale factor relative to size
    let s = size as f32;
    let margin = (s * 0.15) as u32;
    let letter_w = ((s - margin as f32 * 3.0) / 2.0) as u32;
    let letter_h = (s - margin as f32 * 2.0) as u32;
    let stroke = (s * 0.08).max(2.0) as u32;

    // ── Draw "F" ────────────────────────────────────────────────────────────
    let fx = margin;
    let fy = margin;

    // Vertical bar (left side of F)
    for y in fy..fy + letter_h {
        for x in fx..fx + stroke {
            if x < size && y < size {
                img.put_pixel(x, y, green);
            }
        }
    }
    // Top horizontal bar
    for y in fy..fy + stroke {
        for x in fx..fx + letter_w {
            if x < size && y < size {
                img.put_pixel(x, y, green);
            }
        }
    }
    // Middle horizontal bar
    let mid_y = fy + letter_h / 2 - stroke / 2;
    for y in mid_y..mid_y + stroke {
        for x in fx..fx + (letter_w * 3 / 4) {
            if x < size && y < size {
                img.put_pixel(x, y, green);
            }
        }
    }

    // ── Draw "D" ────────────────────────────────────────────────────────────
    let dx = margin * 2 + letter_w;
    let dy = margin;

    // Vertical bar (left side of D)
    for y in dy..dy + letter_h {
        for x in dx..dx + stroke {
            if x < size && y < size {
                img.put_pixel(x, y, green);
            }
        }
    }
    // Top horizontal bar
    for y in dy..dy + stroke {
        for x in dx..dx + letter_w - stroke {
            if x < size && y < size {
                img.put_pixel(x, y, green);
            }
        }
    }
    // Bottom horizontal bar
    for y in (dy + letter_h - stroke)..dy + letter_h {
        for x in dx..dx + letter_w - stroke {
            if x < size && y < size {
                img.put_pixel(x, y, green);
            }
        }
    }
    // Right vertical bar (curved approximation — just a straight bar)
    for y in dy..dy + letter_h {
        for x in (dx + letter_w - stroke)..dx + letter_w {
            if x < size && y < size {
                img.put_pixel(x, y, green);
            }
        }
    }

    // Encode to PNG
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(
            img.as_raw(),
            size,
            size,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap_or_default();

    buf
}

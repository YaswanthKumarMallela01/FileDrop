# FileDrop Version Differences: v0.5.3 vs v0.5.5

This document outlines the differences, optimizations, and changes implemented in FileDrop version `v0.5.5` compared to `v0.5.3`.

---

## 🚀 Key Improvements & Optimizations

### 1. WebAssembly Hashing Integration (`hash-wasm`)
* **Bottleneck Resolved**: Previously, client-side SHA-256 was computed using a pure JavaScript incremental hash class (`SHA256Stream`). While functional, it introduced significant CPU overhead on mobile devices, especially when processing multi-gigabyte files, capping transfer throughput.
* **WebAssembly Speed**: Swapped out the pure JS hasher for `hash-wasm` (v4.11.0), executing hashing via compiled WebAssembly. Hashing overhead is now negligible, freeing the browser main thread to stream data at peak Wi-Fi speeds.
* **100% Offline Compatibility**: Preserving FileDrop's "no cloud, no internet" promise, the `hash-wasm.js` UMD bundle is embedded directly inside the compiled Rust binary and served locally under the `/hash-wasm.js` route. Phones connected to local offline hotspots can download and execute it without any WAN internet connection.

### 2. Real-Time Instantaneous Speed Windowing
* **Issue Resolved**: Previously, speed calculations were computed as a cumulative average since the start of the transfer (`totalSent / elapsed`). Because of WebSocket frame buffering, the transfer speed would start artificially high (e.g. 12+ MB/s while filling up socket buffers) and slowly decay/descend towards the actual physical speed over time (e.g. 5 MB/s). It did not reflect constant real-time speeds.
* **Instantaneous Speed**: Implemented a 1-second moving window for speed calculations on both the browser client (`src/web/index.html`) and the Rust server receiver (`src/transfer/server.rs`). The speed display is now highly responsive, accurate, and remains constant at the actual physical link rate (no slow decay artifacts).

### 3. Streamlined Browser UI code
* **Removed Legacy Code**: Deleted over 100 lines of pure-JS hashing class code from `src/web/index.html`, keeping the single-page application clean and easy to maintain.
* **Modernized Hashing pipeline**: Switched to async promise-based hashing initialization:
  ```javascript
  const hasher = await hashwasm.createSHA256();
  hasher.init();
  ```

---

## 📈 Detailed File Diffs

### Speed Tracking & Hashing Changes

#### [src/web/index.html](file:///d:/AI%20Gen%20Projects/FileDrop/src/web/index.html)
* **Added script tag** to load local WASM hasher:
  ```html
  <script src="/hash-wasm.js"></script>
  ```
* **Updated `sendFile(file, index)`** to use async WASM hasher instead of `SHA256Stream`:
  * Instantiated via `await hashwasm.createSHA256()`
  * Initialized via `hasher.init()`
  * Finalized/hex-digested via `hasher.digest()` instead of `hasher.finalize()`
* **Deleted inline class `SHA256Stream`** in its entirety.
* **Instantaneous Speed Tracker**:
  * Added `lastSpeedUpdate`, `lastBytesSent`, and `currentSpeed` global trackers.
  * Recalculated speed in `updateOverallProgress()` using a 1000ms delta-time window.

#### [src/transfer/server.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/transfer/server.rs)
* Registered the `/hash-wasm.js` GET route in the Axum Router serving the embedded script local asset.
* **ActiveTransfer Struct**: Added `last_speed_update`, `last_bytes_received`, and `current_speed` trackers.
* **Receive Loop**: Updated the progress reporter to calculate speed over a 1-second moving window, updating the TUI in real time.

#### [src/web/mod.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/web/mod.rs)
* Embedded local `hash-wasm.js` file:
  ```rust
  const HASH_WASM_JS: &str = include_str!("hash-wasm.js");
  ```
* Implemented router endpoint handler `serve_hash_wasm()` returning Javascript content type headers.

---

## 🏷️ Version Bumps (`v0.5.3` -> `v0.5.5`)

Version tags were successfully updated in all 13 required locations:
1. **[Cargo.toml](file:///d:/AI%20Gen%20Projects/FileDrop/Cargo.toml)**: `version = "0.5.5"`
2. **[README.md](file:///d:/AI%20Gen%20Projects/FileDrop/README.md)**: Updated version badge, download links, technology table, and feature list.
3. **[build.rs](file:///d:/AI%20Gen%20Projects/FileDrop/build.rs)**: WinRes `ProductVersion` and `FileVersion` metadata.
4. **[docs/README.md](file:///d:/AI%20Gen%20Projects/FileDrop/docs/README.md)**: Badge updated.
5. **[docs/index.html](file:///d:/AI%20Gen%20Projects/FileDrop/docs/index.html)**: Landing page banner, download links, and install steps updated.
6. **[src/discovery/mod.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/discovery/mod.rs)**: Advertise `version = 0.5.5` on local mDNS network.
7. **[src/main.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/main.rs)**: CLI Parser metadata, pairing splash screen, and initialization logs.
8. **[src/share/server.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/share/server.rs)**: Ephemeral share console printout and served page header.
9. **[src/transfer/server.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/transfer/server.rs)**: Health endpoint returns version `0.5.5`.
10. **[src/tui/app.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/tui/app.rs)**: QR connection screen and TUI start log.
11. **[src/tui/share_configurator.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/tui/share_configurator.rs)**: Header version span.
12. **[src/tui/ui.rs](file:///d:/AI%20Gen%20Projects/FileDrop/src/tui/ui.rs)**: ASCII art comment header, main widget title, and setup selection menu header.
13. **[src/web/index.html](file:///d:/AI%20Gen%20Projects/FileDrop/src/web/index.html)**: Logo version title and footer badge.

# FileDrop — Full Feature Expansion Prompt
## For: Antigravity Agent (Google)
## Project: https://github.com/YaswanthKumarMallela01/FileDrop.git

---

## CRITICAL RULES BEFORE YOU START

1. **DO NOT break any existing functionality.** `filedrop receive`, QR scan from phone, file transfer phone→laptop, SHA-256 verification, TUI, mTLS pairing — all must continue working exactly as before.
2. **Read the full existing codebase** before writing a single line of new code.
3. **All new features are additive.** Extend, never replace.
4. Every new feature must have its own subcommand or flag so the existing default flow is untouched.
5. Test each feature independently before combining.

---

## CONTEXT — What FileDrop Currently Does

FileDrop is a Rust CLI tool for secure local Wi-Fi file transfer between a phone and a laptop:
- `filedrop receive` now first prompts the user to choose a connection mode (Router/LAN or Hotspot), then starts an Axum WebSocket server on port 7878, displays a QR code in terminal, and shows a Ratatui TUI dashboard
- Phone scans QR, opens a browser-based web UI (matrix/hacker theme), selects files, and sends them to the laptop
- SHA-256 is computed *while* sending (not before), verified on the laptop side
- Files are saved to the current working directory on the laptop
- Security: X.509 RSA-2048 certs, mTLS for CLI-to-CLI, QR-based pairing
- Tech stack: Rust 2024, Tokio, Axum 0.8, tokio-tungstenite, Ratatui, Crossterm, rustls, rcgen, mdns-sd, sha2, qrcode, Clap v4

---

## FEATURES TO IMPLEMENT

Implement all 6 features below. Each section describes what to build, the exact behavior, file locations, and edge cases to handle.

---

### FEATURE 1 — Single-Device Lock (Fix Existing Security Gap)

**Problem:** Currently, after `filedrop receive` is run and a QR code is displayed, any number of devices on the same network can scan the QR and connect. This is a security gap.

**What to build:**
- After the first WebSocket client successfully connects and sends a `session_hello` or any first message, the server must reject all subsequent WebSocket upgrade requests with HTTP `403 Forbidden` and a JSON body: `{"error": "session_locked", "message": "Another device is already connected."}`
- The QR code display in the terminal must remain visible until the first device connects (current behavior — keep this)
- Once a device connects, print to the TUI log: `[LOCK] Session locked to [device IP]. Further connections rejected.`
- Once the session ends (transfer complete or client disconnects), the lock resets automatically — a new QR code cycle can begin if the user restarts `filedrop receive`
- Add a `--multi` flag to `filedrop receive` that disables the lock for classroom/team use cases (allows N simultaneous connections, current behavior)

**Files to modify:** `src/transfer/server.rs` — add a `AtomicBool` or `Mutex<Option<SocketAddr>>` session lock. Check it in the WebSocket upgrade handler before accepting.

---

### FEATURE 2 — Bidirectional Transfer (Laptop → Phone)

**Problem:** Currently only phone→laptop works. Laptop→phone is not supported.

**What to build:**

#### 2A — New TUI File Browser (Laptop → Phone Push)

Add a new interactive panel to the existing Ratatui TUI that activates when a phone is connected. This panel allows the laptop user to browse and select files/folders to push to the connected phone.

**TUI File Browser behavior:**
- Activate automatically when a phone connects (session lock triggers, device is paired)
- Show a split-pane layout: left pane = existing transfer queue/log (current TUI), right pane = new file browser
- File browser starts at the current working directory
- Display: file/folder name, size (human-readable: KB/MB/GB), type icon (📁 for folder, 📄 for file)
- Navigation keys for the file browser:
  - `Tab` — switch focus between left pane and right file browser pane
  - `↑` / `↓` or `j` / `k` — move cursor in file browser
  - `Enter` — enter directory (if cursor is on a folder)
  - `Backspace` — go up one directory level
  - `Space` — toggle selection on file or folder (multi-select supported)
  - `A` — select all items in current directory
  - `S` — send all selected items to connected phone (triggers push transfer)
  - `Esc` — clear selection
- Selected items shown with a `[✓]` marker and highlighted in green (consistent with existing hacker theme — use the same green `#00FF41`)
- Show selected item count and total size in the pane footer: `Selected: 3 items · 142 MB`
- When a folder is selected, it is automatically zipped on-the-fly before sending (see Feature 2C)

**Files to modify/create:**
- `src/tui/app.rs` — add `FileBrowser` state struct, selection set, current path, focus state
- `src/tui/ui.rs` — add right pane rendering, file list, selection markers, footer
- `src/tui/events.rs` — add Tab, Enter, Backspace, Space, A, S key handlers for file browser mode

#### 2B — Phone-Side Receive UI

The existing phone web UI (`src/web/index.html`) must be extended to also *receive* files pushed from the laptop.

**What to add to the web UI (do not remove anything existing):**
- Add a new section below the existing upload section titled `> INCOMING TRANSFERS`
- When the laptop pushes a file, the browser automatically downloads it using the File API: create a Blob from the binary WebSocket data and trigger `URL.createObjectURL` + programmatic `<a download>` click
- Show a progress bar for each incoming file (identical styling to the existing upload progress bar — green on black, monospace font)
- After download completes, show: `✓ [filename] saved to Downloads`
- Files are always saved to the phone's default Downloads folder (this is browser-enforced behavior — no action needed, just document it in comments)
- No new buttons needed — downloads are triggered automatically by the server push

**Wire protocol extension (add to `src/transfer/protocol.rs`):**

Add these new message types (alongside existing ones, do not remove existing types):
```json
// Laptop → Phone (server push direction)
{"type": "push_start", "name": "report.pdf", "size": 5242880, "sha256": "streaming", "transfer_id": "uuid-v4"}
// Binary chunks follow (same 1MB chunk format as existing)
{"type": "push_done", "checksum": "sha256:abc...", "transfer_id": "uuid-v4"}

// Phone → Laptop (acknowledgment)
{"type": "push_ack", "success": true, "transfer_id": "uuid-v4"}
```

**Files to modify:** `src/transfer/protocol.rs`, `src/transfer/server.rs`, `src/web/index.html`

#### 2C — On-the-Fly Folder Zipping

When the user selects a folder in the TUI file browser and presses `S` to send:
- Create a ZIP archive of the folder in memory using a streaming approach (do not load entire folder into RAM)
- Use the `zip` crate (add to `Cargo.toml`: `zip = "2"`)
- Name the zip: `[foldername]_filedrop_[timestamp].zip` e.g. `Documents_filedrop_20250530_143022.zip`
- Stream the ZIP directly into the WebSocket as binary chunks — do not write a temp file to disk
- Show in TUI log: `[ZIP] Compressing Documents/ on the fly...` then `[PUSH] Sending Documents_filedrop_20250530_143022.zip (142 MB)`
- On the phone side, the browser downloads the ZIP file normally

**Files to create:** `src/transfer/zipper.rs` — async streaming zip builder
**Files to modify:** `src/transfer/server.rs` — integrate zipper into push flow

---

### FEATURE 3 — Connection Mode Selection Prompt + Hotspot Mode

**Problem:** FileDrop currently assumes both devices are on the same router-based Wi-Fi. Users have no guidance on which connection method to use, and in router-less environments (airplane, car, remote location) FileDrop silently fails with no helpful direction.

**Solution:** Every time `filedrop receive` is run, display an **interactive connection mode selection screen** in the terminal *before* starting the server. The user picks their connection method; FileDrop then configures itself accordingly and proceeds.

---

#### 3A — Connection Mode Selection Screen (runs at start of `filedrop receive`)

**What to build:**

Immediately after `filedrop receive` is invoked, before any server starts, render an interactive full-terminal selection screen using Ratatui (consistent with the existing hacker theme: black background, green `#00FF41` text, box-drawing characters, monospace font).

**Screen layout:**
```
╔══════════════════════════════════════════════════════╗
║              [ FILEDROP ] CONNECTION SETUP           ║
╠══════════════════════════════════════════════════════╣
║                                                      ║
║  Select how your devices will connect:               ║
║                                                      ║
║  ┌────────────────────────────────────────────────┐  ║
║  │  [1]  ROUTER / LAN  (recommended)              │  ║
║  │       Both devices on same Wi-Fi network.      │  ║
║  │       No internet required.                    │  ║
║  └────────────────────────────────────────────────┘  ║
║                                                      ║
║  ┌────────────────────────────────────────────────┐  ║
║  │  [2]  HOTSPOT  (no router needed)              │  ║
║  │       Airplane, car, remote location.          │  ║
║  │       One device creates a Wi-Fi hotspot.      │  ║
║  └────────────────────────────────────────────────┘  ║
║                                                      ║
║  Navigate: ↑/↓ or 1/2     Confirm: Enter            ║
║  Skip (use last choice):   S                         ║
╚══════════════════════════════════════════════════════╝
```

**Interaction behavior:**
- `↑` / `↓` or pressing `1` / `2` — move highlight between the two options
- Currently highlighted option rendered with bright green border and a `►` indicator
- `Enter` — confirm selection and proceed
- `S` — skip prompt and use the last saved choice (stored in `~/.config/filedrop/config.toml` as `last_connection_mode = "router"` or `"hotspot"`)
- If no prior choice exists and `S` is pressed, default to Router mode and show: `[INFO] Defaulting to Router/LAN mode`
- The selection screen must clear itself from the terminal before the QR code and TUI appear — do not leave it behind

**Persist the choice:** After the user selects, write it to `~/.config/filedrop/config.toml`:
```toml
last_connection_mode = "router"   # or "hotspot"
```
Use the existing `src/config.rs` pattern for reading/writing config.

**Skip flag:** Add `--mode <router|hotspot>` flag to `filedrop receive` to bypass the prompt entirely:
- `filedrop receive --mode router` — skip prompt, go straight to router mode
- `filedrop receive --mode hotspot` — skip prompt, go straight to hotspot mode
- Useful for scripting or power users who don't want the interactive screen

---

#### 3B — Router / LAN Mode Flow (Option 1)

After the user selects Router/LAN:
1. Clear the selection screen
2. Print a one-line confirmation: `[MODE] Router/LAN — connecting via shared Wi-Fi network`
3. Proceed with the **exact existing `filedrop receive` flow** — server start, QR code, TUI. Zero changes to this path.
4. Auto-detect network interfaces and pick the best non-loopback IP to show in the QR code URL (existing behavior — keep it)
5. **If no valid network interface is found** (no active Wi-Fi/Ethernet IP other than loopback): do NOT silently fail. Print inside the TUI log: `[WARN] No active network detected. Is your Wi-Fi connected? Or try: filedrop receive --mode hotspot`

---

#### 3C — Hotspot Mode Flow (Option 2)

After the user selects Hotspot:
1. Clear the selection screen
2. Detect the current OS (Windows / macOS / Linux)
3. Render a **hotspot setup guide screen** in the terminal (same hacker theme styling):

```
╔══════════════════════════════════════════════════════╗
║           [ FILEDROP ] HOTSPOT SETUP                 ║
╠══════════════════════════════════════════════════════╣
║                                                      ║
║  Enable a Wi-Fi hotspot on THIS laptop:              ║
║                                                      ║
║  ── Windows ──────────────────────────────────────  ║
║  Settings → Network → Mobile Hotspot → Turn On      ║
║  Or run in Admin PowerShell:                         ║
║  > netsh wlan start hostednetwork                    ║
║                                                      ║
║  ── macOS ─────────────────────────────────────────  ║
║  System Settings → General → Sharing →               ║
║  Internet Sharing → Wi-Fi → On                       ║
║                                                      ║
║  ── Linux ─────────────────────────────────────────  ║
║  > nmcli device wifi hotspot ssid FileDrop           ║
║    ifname wlan0 password filedrop123                 ║
║  (Auto-setup available: press A)                     ║
║                                                      ║
║  Then connect your PHONE to the hotspot Wi-Fi.       ║
║                                                      ║
║  [A] Auto-setup (Linux only)   [Y] Done, continue   ║
║  [B] Back to mode selection                          ║
╚══════════════════════════════════════════════════════╝
```

Show only the section relevant to the detected OS — hide the other two OS sections. All three sections shown above are for illustration; at runtime only one OS block is displayed.

**Key actions on this screen:**
- `Y` / `Enter` — user confirms hotspot is active; proceed to start `filedrop receive` with a hotspot-aware label
- `A` — Linux only: run `nmcli device wifi hotspot ifname wlan0 ssid FileDrop password filedrop123` automatically via `std::process::Command`. Show spinner: `[HOTSPOT] Creating hotspot...`. On success show: `[HOTSPOT] ✓ SSID: FileDrop  Password: filedrop123`. Then auto-continue to receiver after 2 seconds.
- `B` — go back to the connection mode selection screen (3A)

4. After confirmation, start `filedrop receive` — full existing flow — but add a persistent label in the TUI header area: `[HOTSPOT MODE]` in yellow `#FFD700` so the user always knows which mode is active.
5. Print above the QR code: `[HOTSPOT MODE] Connect your phone to the hotspot Wi-Fi first, then scan this QR`
6. After hotspot session ends (user quits), optionally print: `[INFO] To stop the hotspot: nmcli connection delete FileDrop` (Linux only)

---

#### 3D — `filedrop hotspot` Standalone Command (Shortcut)

Keep `filedrop hotspot` as a direct shortcut command that jumps straight to the hotspot setup guide screen (3C) without showing the mode selection screen first. Equivalent to `filedrop receive --mode hotspot`.

```
filedrop hotspot          # Jump straight to hotspot setup guide, then start receiver
filedrop hotspot --auto   # Linux only: auto-create hotspot via nmcli, then start receiver
```

**Files to create:** `src/hotspot/mod.rs` — OS detection, guide screen rendering, nmcli integration
**Files to modify:**
- `src/main.rs` — add `hotspot` subcommand, add `--mode` flag to `receive` subcommand
- `src/tui/app.rs` — add `ConnectionModeScreen` and `HotspotGuideScreen` states to the app state machine
- `src/tui/ui.rs` — render both new screens (mode selection + hotspot guide)
- `src/tui/events.rs` — handle `1`, `2`, `A`, `Y`, `B`, `S` keypresses on the new screens
- `src/config.rs` — add `last_connection_mode` field to config struct
- `src/transfer/server.rs` — add network interface detection warning for router mode

---

### FEATURE 4 — Ephemeral Sharing Links (`filedrop share`)

**What to build:**

**`filedrop share <file_or_folder> [options]` command:**

Options:
- `--expires <duration>` — e.g. `10m`, `1h`, `30s`. Default: `15m`
- `--once` — link expires after first download (regardless of time)
- `--pin <4-digit-number>` — optional PIN protection shown on phone web UI before download

**Behavior:**
1. Start a minimal Axum HTTP server on port 7879 (separate from main transfer port 7878)
2. Generate a cryptographically random 8-character token (alphanumeric, URL-safe) using the `rand` crate
3. Display a QR code in terminal pointing to `http://[laptop-ip]:7879/get/[token]`
4. Display a text summary box:
   ```
   ┌─────────────────────────────────────┐
   │  EPHEMERAL SHARE ACTIVE             │
   │  File:    quarterly_report.pdf      │
   │  Expires: 15 minutes                │
   │  Access:  1 download (--once)       │
   │  PIN:     4829                      │
   │  URL:     http://192.168.1.5:7879/  │
   │           get/xK9mP2qR              │
   └─────────────────────────────────────┘
   ```
5. When phone visits the URL:
   - If `--pin` was set: show a PIN entry screen (styled in the existing matrix/hacker theme) before allowing download
   - Show a simple download page: filename, size, a single `▶ DOWNLOAD` button
   - Clicking download streams the file directly (for folders: zip on-the-fly using the zipper from Feature 2C)
6. After expiry or after `--once` download completes: server shuts down, print `[SHARE] Link expired. Session closed.` to terminal
7. Press `Q` or `Ctrl+C` to manually kill the share server

**SHA-256 still applies:** compute hash of the shared file at share-start, display it in the terminal share summary so the recipient can verify

**Files to create:** `src/share/mod.rs`, `src/share/server.rs`
**Files to modify:** `src/main.rs` — add `share` subcommand; `Cargo.toml` — add `rand` crate if not present

---

### FEATURE 5 — LAN Folder Sync (`filedrop sync`)

**What to build:**

**`filedrop sync <local_folder> --with <peer_ip>` command** (and corresponding `filedrop sync --listen` on the other device)

This is a **one-way, push-based, real-time sync** from source to destination over LAN. It is NOT a full bidirectional Dropbox clone — keep scope focused.

**Behavior:**
1. On the source device: `filedrop sync ~/Documents/ProjectX --with 192.168.1.5`
2. On the destination device: `filedrop sync --listen --save ~/Documents/ProjectX-sync`
3. Source watches the folder using the `notify` crate (cross-platform file watcher, add to `Cargo.toml`)
4. On any file change (create, modify, delete):
   - **Create/Modify:** Send the changed file over WebSocket using the existing chunked + SHA-256 transfer protocol (reuse `src/transfer/chunker.rs` and `protocol.rs`)
   - **Delete:** Send a `{"type": "sync_delete", "path": "relative/path/to/file.txt"}` control message
   - **New folder:** Send a `{"type": "sync_mkdir", "path": "relative/path/to/folder"}` control message
5. On initial connect, perform a **full sync scan**: compare file lists (name + size + SHA-256) and transfer only files that differ. Do NOT transfer files that are identical.
6. Show a live Ratatui TUI on both ends showing:
   - Sync status: `WATCHING` / `SYNCING` / `IDLE`
   - Recent changes list (scrollable): `[SYNC] Modified: src/main.rs (4.2 KB) → sent`
   - Connection status and peer IP
7. Press `Q` to stop sync (does not delete any already-synced files)

**Conflict handling (keep it simple):** Last-write-wins. If destination has a newer version, it gets overwritten. Print a warning: `[WARN] Overwriting newer file: report.pdf`

**Port:** Use port 7880 for sync (separate from 7878 and 7879)

**Files to create:** `src/sync/mod.rs`, `src/sync/watcher.rs`, `src/sync/server.rs`
**Files to modify:** `src/main.rs` — add `sync` subcommand; `Cargo.toml` — add `notify = "6"` crate

---

### FEATURE 6 — Progressive Web App (PWA) for Phone Web UI

**What to build:**

Convert the existing phone browser web UI (`src/web/index.html`) into an installable PWA. This allows the phone to save FileDrop as a home screen app — no QR scan needed on repeat uses.

**Changes to `src/web/index.html`:**
1. Add a `<link rel="manifest" href="/manifest.json">` tag in `<head>`
2. Add a `<meta name="theme-color" content="#000000">` tag (black, consistent with matrix theme)
3. Add a `<script>` block at the bottom that registers a Service Worker: `navigator.serviceWorker.register('/sw.js')`
4. Add an "Install App" button that appears only when the browser fires the `beforeinstallprompt` event. Style it consistently with the existing hacker UI (green border, monospace font, same button style as existing `▶ EXECUTE TRANSFER` button). Button text: `⬇ INSTALL FILEDROP`

**New routes to add in `src/web/mod.rs` and `src/transfer/server.rs`:**

Serve these new static files at their respective paths:

**`GET /manifest.json`:**
```json
{
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
}
```

**`GET /sw.js`** — Service Worker:
```javascript
const CACHE = 'filedrop-v1';
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
});
```

**`GET /icon-192.png` and `GET /icon-512.png`:**
Generate two simple PNG icons programmatically using the `image` crate (add to `Cargo.toml`). The icon should be:
- Black background (`#000000`)
- Green text (`#00FF41`) showing `FD` in a monospace-style pixel layout
- Simple enough to generate with basic `image` crate rectangle/pixel operations — no external image files needed

**Files to modify:** `src/web/index.html`, `src/web/mod.rs`, `src/transfer/server.rs`
**Files to create:** Icon generation logic inline in `src/web/mod.rs`
**Cargo.toml additions:** `image = "0.25"`

---

### FEATURE 7 — End-to-End Encrypted Transfers (Optional Layer)

**What to build:**

Add AES-256-GCM encryption as an optional layer for file payload. Activated with `--encrypt` flag on `filedrop receive`.

**`filedrop receive --encrypt` behavior:**
1. At session start, derive a shared AES-256 session key using ECDH (Elliptic Curve Diffie-Hellman):
   - Laptop generates an ephemeral ECDH keypair (use `p256` crate or `ring` crate)
   - Laptop's public key is embedded in the QR code URL as a query parameter: `http://192.168.1.5:7878/?pubkey=base64encodedkey`
   - Phone JavaScript receives the public key from the URL, generates its own ephemeral ECDH keypair using `window.crypto.subtle.generateKey` (WebCrypto API — no library needed, built into all modern browsers)
   - Both sides derive the same shared secret via ECDH, then derive an AES-256-GCM key from it using HKDF
2. All binary file chunks are encrypted with AES-256-GCM before sending (phone side, in JavaScript using `window.crypto.subtle.encrypt`)
3. Laptop decrypts each chunk as it arrives before writing to disk
4. SHA-256 verification happens on the **decrypted** data (not the encrypted chunks)
5. Show `[ENC]` badge in TUI log next to encrypted transfers: `[ENC] ✓ photo.jpg (5.2 MB) received`
6. If `--encrypt` is NOT passed, behavior is identical to current (no encryption, no change)

**Crates to add:** `p256 = "0.13"` or `ring = "0.17"` (use whichever is simpler for ECDH + HKDF)

**Files to modify:** `src/transfer/server.rs`, `src/transfer/protocol.rs`, `src/transfer/chunker.rs`, `src/web/index.html`

---

## FOLDER SELECTION FROM PHONE (Browser Limitation + Workaround)

**Problem:** Mobile browsers support `<input type="file" webkitdirectory>` on some Android browsers (Chrome) but NOT on iOS Safari. It is inconsistent.

**Solution to implement in `src/web/index.html`:**

1. **Try native folder selection first:**
   Add a second button below the existing file picker: `> SELECT FOLDER`
   ```html
   <input type="file" id="folderInput" webkitdirectory multiple style="display:none">
   <button onclick="document.getElementById('folderInput').click()"> > SELECT FOLDER</button>
   ```
   If the browser supports `webkitdirectory` (Android Chrome), this opens a folder picker and selects all files inside recursively.

2. **Fallback — ZIP prompt:**
   Before sending, detect if multiple files share a common folder prefix (i.e., they came from a folder selection). If yes, and if `window.JSZip` is available, zip them client-side using JSZip (load from CDN: `https://cdnjs.cloudflare.com/ajax/libs/jszip/3.10.1/jszip.min.js`).
   - Show a toast notification: `[ZIP] Compressing folder for transfer...` with a spinner
   - After zipping, send the resulting ZIP as a single file using the existing transfer flow
   - Name it: `[foldername]_filedrop.zip`

3. **iOS fallback message:**
   If the folder input is clicked and `webkitdirectory` is not supported (iOS Safari), show an inline message in the UI: `iOS does not support folder selection. Please zip your folder first, then use > SELECT FILES.`
   Style this message in yellow (`#FFD700`) to stand out from the green theme.

**Files to modify:** `src/web/index.html` only
**CDN addition:** JSZip from cdnjs (already whitelisted on most networks)

---

## Cargo.toml — All New Dependencies Summary

Add these to `[dependencies]` (check for version conflicts with existing deps first):

```toml
zip = "2"                    # Feature 2C: on-the-fly folder zipping
rand = "0.8"                 # Feature 4: ephemeral token generation
notify = "6"                 # Feature 5: file system watcher for sync
image = "0.25"               # Feature 6: PWA icon generation
p256 = "0.13"                # Feature 7: ECDH key exchange (or use `ring`)
# ring = "0.17"              # Alternative to p256 for crypto
uuid = { version = "1", features = ["v4"] }  # Feature 4: transfer_id generation
```

---

## New CLI Commands Summary

After implementation, `filedrop --help` should show:

```
Commands:
  receive          Start receiver (shows connection mode prompt first) [--mode <router|hotspot>] [--multi] [--encrypt]
  send <path>      Send file/directory to a discovered peer
  share <path>     Create ephemeral one-time share link [--expires <duration>] [--once] [--pin <pin>]
  sync <folder>    Watch and sync folder to paired device [--with <ip>] [--listen] [--save <path>]
  hotspot          Guide to create direct device connection (no router needed) [--auto]
  pair             Generate QR code for certificate pairing
  peers            List all paired devices
  unpair <n>       Remove a paired device
  demo             Visual TUI test with simulated transfers
```

---

## Testing Checklist (Run After Implementation)

- [ ] `filedrop receive` — connection mode selection screen appears first, styled correctly in hacker theme
- [ ] Mode selection: `1`/`2` keys and `↑`/`↓` navigation highlight the correct option
- [ ] Mode selection: `S` skips prompt using last saved mode from config; defaults to Router if no prior choice
- [ ] `filedrop receive --mode router` — skips prompt, goes directly to router mode
- [ ] `filedrop receive --mode hotspot` — skips prompt, goes directly to hotspot guide screen
- [ ] Router mode: existing phone→laptop transfer still works, QR shows, SHA-256 verified
- [ ] Router mode: `[WARN]` printed in TUI if no active network interface detected
- [ ] Hotspot mode: correct OS-specific instructions shown (only the current OS section visible)
- [ ] Hotspot mode Linux `A` key: nmcli creates hotspot automatically, SSID/password printed, auto-continues
- [ ] Hotspot mode `B` key: returns to mode selection screen cleanly
- [ ] Hotspot mode: `[HOTSPOT MODE]` yellow label visible in TUI header during session
- [ ] `filedrop hotspot` standalone command: jumps directly to hotspot guide screen
- [ ] `filedrop hotspot --auto`: Linux only, auto-creates hotspot and starts receiver without prompts
- [ ] `filedrop receive` — second device connecting gets 403 after first device connects
- [ ] `filedrop receive --multi` — multiple devices can connect simultaneously
- [ ] TUI file browser opens when phone connects, Tab switches focus, Space selects files
- [ ] Selecting a folder in TUI and pressing S sends a ZIP to phone's Downloads
- [ ] Phone UI shows incoming file progress and auto-downloads to Downloads folder
- [ ] `filedrop share report.pdf --expires 10m --once --pin 4829` — QR shows, PIN screen on phone, file downloads, link dies after download
- [ ] `filedrop sync ~/TestFolder --with <peer_ip>` — file changes sync in real-time to destination
- [ ] `filedrop hotspot` — correct OS-specific instructions shown
- [ ] Phone web UI shows "Install App" button on Chrome Android, installs as PWA
- [ ] `filedrop receive --encrypt` — transfers complete successfully, `[ENC]` shows in TUI
- [ ] Folder select button appears on phone web UI; falls back to ZIP on iOS
- [ ] All existing commands (`send`, `pair`, `peers`, `unpair`, `demo`) still work

---

## Code Style & Conventions (Match Existing Codebase)

- All async code uses `tokio::spawn` and `async/await` — no blocking calls on async threads
- Errors use `anyhow::Result` or `thiserror` — match whichever the existing code uses
- TUI colors: background `#000000`, primary text `#00FF41`, warnings `#FFD700`, errors `#FF0000`
- All log messages in TUI follow the format: `[TAG] Message text` e.g. `[LOCK]`, `[PUSH]`, `[SYNC]`, `[ENC]`, `[ZIP]`
- WebSocket message parsing: always match on the `type` field of JSON control messages
- Prefer streaming over buffering for all file operations — never load a full file into memory

---

*End of prompt. Implement features in order: 1 → 2A → 2B → 2C → 3 → 4 → 5 → 6 → 7. Commit after each feature passes its tests.*

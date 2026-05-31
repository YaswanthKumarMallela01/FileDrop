<p align="center">
  <img src="https://img.shields.io/badge/Version-0.3.2-00e87b?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/RUST-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/WebSocket-010101?style=for-the-badge&logo=socketdotio&logoColor=00FF41" alt="WebSocket">
  <img src="https://img.shields.io/badge/TUI-Terminal-00FF41?style=for-the-badge" alt="TUI">
  <img src="https://img.shields.io/badge/License-MIT-yellow?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS-blue?style=for-the-badge" alt="Platform">
</p>

<h1 align="center">
  <code>[ FILEDROP ]</code>
  <br>
  <sub>Secure Local Wi-Fi File Transfer · Phone ↔ Laptop</sub>
</h1>

<p align="center">
  <b>Transfer files between your phone and laptop at full Wi-Fi speed.</b><br>
  No cloud. No internet. No app install. No file size limit. Just open a URL.
</p>

<p align="center">
  <a href="https://github.com/YaswanthKumarMallela01/FileDrop/releases/tag/v0.3.2"><b>📦 Download v0.3.2</b></a> &nbsp;·&nbsp;
  <a href="https://yaswanthkumarmallela01.github.io/FileDrop/"><b>🌐 Website</b></a> &nbsp;·&nbsp;
  <a href="https://github.com/YaswanthKumarMallela01/FileDrop/issues"><b>🐛 Report Bug</b></a>
</p>

---

## ⚡ What Makes FileDrop Different?

| Feature | FileDrop | AirDrop | SHAREit | Bluetooth |
|---------|----------|---------|---------|-----------|
| **Cross-platform** | ✅ Any device with a browser | ❌ Apple only | ⚠️ App required | ✅ |
| **No app needed** | ✅ Just open a URL | ❌ | ❌ | ✅ |
| **Large files (10GB+)** | ✅ Streaming, no RAM limit | ✅ | ⚠️ | ❌ Very slow |
| **No cloud/internet** | ✅ 100% local | ✅ | ❌ Routes through cloud | ✅ |
| **SHA-256 verification** | ✅ Every file verified | ❌ | ❌ | ❌ |
| **Speed** | 🚀 Full Wi-Fi speed | 🚀 | ⚠️ | 🐌 |
| **Open source** | ✅ MIT License | ❌ | ❌ | N/A |
| **Privacy** | ✅ No data leaves LAN | ✅ | ❌ Ads + tracking | ✅ |

---

## 🚀 Quick Start

### ⬇️ Download Pre-Built Binary (Recommended)

No Rust or build tools needed. Download the single executable for your platform:

| Platform | Download | Size |
|----------|----------|------|
| **Windows** | [filedrop-windows-amd64.exe](https://github.com/YaswanthKumarMallela01/FileDrop/releases/download/v0.3.2/filedrop-windows-amd64.exe) | ~7.3 MB |
| **Linux** | [filedrop-linux-amd64](https://github.com/YaswanthKumarMallela01/FileDrop/releases/download/v0.3.2/filedrop-linux-amd64) | ~6.5 MB |
| **macOS** | [filedrop-macos-amd64](https://github.com/YaswanthKumarMallela01/FileDrop/releases/download/v0.3.2/filedrop-macos-amd64) | ~5.9 MB |

#### Windows
1. Download and **double-click** `filedrop-windows-amd64.exe`
2. An interactive menu appears — select **"Install FileDrop (Add to System PATH)"**
3. Open a **new** terminal and type `filedrop receive`

#### Linux / macOS
```bash
# Linux
wget https://github.com/YaswanthKumarMallela01/FileDrop/releases/download/v0.3.2/filedrop-linux-amd64
chmod +x filedrop-linux-amd64
sudo mv filedrop-linux-amd64 /usr/local/bin/filedrop

# macOS
curl -LO https://github.com/YaswanthKumarMallela01/FileDrop/releases/download/v0.3.2/filedrop-macos-amd64
chmod +x filedrop-macos-amd64
sudo mv filedrop-macos-amd64 /usr/local/bin/filedrop
```

### 🔧 Build From Source (Requires Rust)

```bash
cargo install --git https://github.com/YaswanthKumarMallela01/FileDrop.git
```

Or clone and build:

```bash
git clone https://github.com/YaswanthKumarMallela01/FileDrop.git
cd FileDrop
cargo install --path .
```

### Receive Files (On Laptop)

```bash
# Navigate to where you want files saved
cd ~/Downloads

# Start the receiver
filedrop receive
```

A **QR code** appears in your terminal and **stays until you press Enter**.

### Send Files (From Phone)

1. **Scan the QR code** with your phone camera
2. A hacker-themed web interface opens in your browser
3. Tap **"> SELECT FILES"** → choose files (any size!)
4. Tap **"▶ EXECUTE TRANSFER"**
5. Watch real-time progress with ETA on both devices

> ⚠️ Both devices must be on the **same Wi-Fi network**.

---

## 🛠️ All Commands

```
filedrop receive     Start server to receive files into the current directory
filedrop share       Open an interactive TUI to select files to share via an ephemeral link (with PIN/expiry)
filedrop sync        Sync a folder in real-time across devices
filedrop send <path> Send file/directory to a discovered peer
filedrop pair        Generate QR code for certificate pairing
filedrop peers       List all paired devices
filedrop unpair <n>  Remove a paired device
filedrop hotspot     Interactive guide to set up a direct Hotspot connection
filedrop demo        Visual TUI test with simulated transfers
filedrop --help      Show all commands
```

### TUI Keyboard Controls

FileDrop features an interactive v2 Terminal UI (TUI) for receiving and sharing files.

| Key | Action |
|-----|--------|
| `↑` / `↓` / `j` / `k` | Scroll logs, or navigate files in the share configurator |
| `Space` / `Enter` | Select/toggle files for sharing |
| `Tab` | Switch input fields (PIN, expiry, etc.) |
| `PgUp` / `PgDn` | Scroll system log (10 lines at a time) |
| `H` / `Home` | Jump to top of queue |
| `E` / `End` | Jump to bottom of queue |
| `Q` / `Esc` | Quit |
| `Ctrl+C` | Abort transfer and exit |

---

## 🔒 Security Architecture

FileDrop is built with security-first design. Your files **never leave your local network**.

### How It Works

```
┌─────────────────────────────────────────────────┐
│  YOUR LOCAL WI-FI NETWORK                        │
│                                                   │
│  📱 Phone ──── WebSocket ────→ 💻 Laptop         │
│        (browser)         (port 7878)             │
│                                                   │
│  ✅ Data stays on YOUR network                    │
│  ✅ No internet connectivity required             │
│  ✅ No cloud server involved                      │
│  ✅ No third-party can intercept                  │
└─────────────────────────────────────────────────┘
```

### Security Features

| Layer | Protection | Description |
|-------|-----------|-------------|
| **Integrity** | SHA-256 | Every file is hashed on both ends. Server rejects files with mismatched checksums |
| **Verification** | Dual-hash | Client computes hash while sending; server computes independently and compares |
| **Network** | LAN-only | Server binds to `0.0.0.0` but only works on local network (not exposed to internet) |
| **Transport** | mTLS Ready | Certificate-based mutual authentication for CLI-to-CLI transfers |
| **Identity** | X.509 Certs | RSA-2048 certificates generated locally with SHA-256 fingerprinting |
| **Pairing** | QR Code | One-time certificate exchange via QR code — no passwords transmitted |
| **Storage** | Secure dirs | Certificates stored in OS-specific app directories with restricted permissions |
| **Protocol** | JSON+Binary | Typed control messages prevent injection attacks |

### What We Don't Do (By Design)

- ❌ **No cloud relay** — Files are never uploaded to any server
- ❌ **No analytics/tracking** — Zero telemetry, zero phone-home
- ❌ **No account required** — No sign-up, no login
- ❌ **No ads** — Open source, forever free
- ❌ **No file metadata leaks** — Only filename, size, and hash are transmitted

---

## ⚡ Performance

### Speed Optimizations

FileDrop v0.1.0 includes several optimizations for large file transfers:

| Optimization | Impact | Details |
|-------------|--------|---------|
| **Hash-while-sending** | **~50% faster** | SHA-256 computed alongside transfer, not before |
| **1MB chunks** | **~4x less overhead** | Reduced from 256KB to 1MB WebSocket frames |
| **4MB write buffer** | **Fewer disk I/Os** | Buffered writes reduce syscall overhead |
| **Streaming SHA-256** | **No RAM limit** | Files >10GB supported without loading into memory |
| **Backpressure control** | **No data loss** | WebSocket buffer monitoring prevents overflow |

### Expected Speeds

| Network | Expected Speed | 19GB Transfer Time |
|---------|---------------|-------------------|
| Wi-Fi 5 (802.11ac) | 20-50 MB/s | ~6-15 min |
| Wi-Fi 6 (802.11ax) | 50-100 MB/s | ~3-6 min |
| Wi-Fi 6E | 100-200 MB/s | ~1.5-3 min |
| 5GHz band | Generally faster than 2.4GHz | Use 5GHz when possible |

> **Tip**: If transfers are slow, make sure both devices are on the **5GHz Wi-Fi band**, not 2.4GHz. 2.4GHz tops out at ~5-10 MB/s in practice.

---

## 🏗️ Architecture

```
                    ┌─────────────────────────────┐
                    │   📱 Phone Browser           │
                    │                               │
                    │  ┌─────────────────────────┐  │
                    │  │  FileDrop Web UI         │  │
                    │  │  • Drag & drop picker    │  │
                    │  │  • Streaming SHA-256     │  │
                    │  │  • 1MB chunked send      │  │
                    │  │  • Hash-while-sending    │  │
                    │  │  • Matrix rain theme ✦   │  │
                    │  └───────────┬─────────────┘  │
                    └──────────────┼─────────────────┘
                                   │ WebSocket (ws://)
                                   │ JSON control + Binary chunks
                    ┌──────────────┼─────────────────┐
                    │  💻 Laptop    │                  │
                    │              ▼                   │
                    │  ┌─────────────────────────┐    │
                    │  │  Axum HTTP Server :7878  │    │
                    │  │  GET /    → Web UI       │    │
                    │  │  GET /ws  → Transfer     │    │
                    │  └───────────┬─────────────┘    │
                    │              │                   │
                    │  ┌───────────▼─────────────┐    │
                    │  │  WebSocket Handler       │    │
                    │  │  • Parse JSON control    │    │
                    │  │  • 4MB buffered write    │    │
                    │  │  • SHA-256 verify        │    │
                    │  │  • Checksum NACK/ACK     │    │
                    │  └───────────┬─────────────┘    │
                    │              │                   │
                    │  ┌───────────▼─────────────┐    │
                    │  │  Ratatui TUI Dashboard   │    │
                    │  │  • Scrollable queue      │    │
                    │  │  • Transfer log          │    │
                    │  │  • ETA + speed history   │    │
                    │  └─────────────────────────┘    │
                    └─────────────────────────────────┘
```

### Wire Protocol

```
Phone → Laptop:
  1. JSON:   {"type":"file_start", "name":"photo.jpg", "size":5242880, "sha256":"streaming"}
  2. Binary: [1MB chunk] [1MB chunk] [1MB chunk] ...  (hash computed simultaneously)
  3. JSON:   {"type":"file_done", "checksum":"sha256:abc..."}  (final hash)

Laptop → Phone:
  4. JSON:   {"type":"file_ack", "success":true}  (or false if checksum mismatch)
```

---

## 🔧 Windows Firewall (One-Time Setup)

Windows blocks incoming connections by default. Open an **Admin PowerShell** and run:

```powershell
netsh advfirewall firewall add rule name="FileDrop" dir=in action=allow protocol=TCP localport=7878 profile=any
```

---

## 📁 Project Structure

```
FileDrop/
├── Cargo.toml                    Dependencies & build config
├── README.md                     This file
└── src/
    ├── main.rs            ─────  CLI entry point + QR code display
    ├── config.rs          ─────  TOML config (~/.config/filedrop/)
    ├── security/
    │   ├── certs.rs       ─────  X.509 RSA-2048 certificates (rcgen)
    │   └── pairing.rs     ─────  QR code pairing flow
    ├── transfer/
    │   ├── protocol.rs    ─────  Wire protocol + JSON messages
    │   ├── chunker.rs     ─────  SHA-256 verification + 4MB buffered I/O
    │   ├── server.rs      ─────  Axum WebSocket server + web routes
    │   └── client.rs      ─────  WebSocket send client
    ├── discovery/
    │   └── mod.rs         ─────  mDNS (Bonjour) advertisement
    ├── tui/
    │   ├── app.rs         ─────  State management + event loop
    │   ├── events.rs      ─────  Keyboard input (scroll, quit, home/end)
    │   └── ui.rs          ─────  Ratatui rendering (hacker theme + ETA)
    └── web/
        ├── mod.rs         ─────  Embedded HTML server
        └── index.html     ─────  Mobile web UI (matrix rain + streaming hash)
```

---

## 🧰 Tech Stack

| Component | Technology |
|-----------|-----------|
| Language | Rust 2024 edition |
| Async Runtime | Tokio (multi-threaded) |
| HTTP Server | Axum 0.8 |
| WebSocket | tokio-tungstenite |
| TUI Framework | Ratatui + Crossterm |
| TLS | rustls + rcgen |
| Discovery | mdns-sd |
| Hashing | SHA-256 (sha2 crate + pure JS streaming) |
| QR Code | qrcode crate (terminal display) |
| CLI Parser | Clap v4 |
| Disk I/O | 4MB BufWriter for optimized writes |

---

## 🤝 Contributing

```bash
git clone https://github.com/YaswanthKumarMallela01/FileDrop.git
cd FileDrop
cargo build
cargo test
cargo run -- demo    # Visual test
cargo run -- receive # Live test
```

---

## 📄 License

MIT License — use it however you want.

---

<p align="center">
  <sub>Built with 🦀 Rust · No cloud · No trace · Just raw speed</sub>
  <br>
  <sub>Designed by <a href="https://github.com/YaswanthKumarMallela01">Yaswanth Kumar Mallela</a></sub>
</p>

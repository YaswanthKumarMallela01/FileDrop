<p align="center">
  <img src="https://img.shields.io/badge/RUST-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/WebSocket-010101?style=for-the-badge&logo=socketdotio&logoColor=00FF41" alt="WebSocket">
  <img src="https://img.shields.io/badge/TUI-Terminal-00FF41?style=for-the-badge" alt="TUI">
  <img src="https://img.shields.io/badge/License-MIT-yellow?style=for-the-badge" alt="License">
</p>

<h1 align="center">
  <br>
  <code>[ FILEDROP ]</code>
  <br>
  <sub>Secure Local Wi-Fi File Transfer</sub>
</h1>

<p align="center">
  <b>Send files from your phone to your laptop in seconds.</b><br>
  No cloud. No internet. No apps to install. Just open a URL.
</p>

<br>

```
  ╔══════════════════════════════════════════════════════╗
  ║  [FILEDROP]  v0.1.0  ::  RECEIVE_MODE              ║
  ╠══════════════════════════════════════════════════════╣
  ║                                                      ║
  ║  > SCAN QR CODE ON PHONE TO CONNECT:                 ║
  ║                                                      ║
  ╚══════════════════════════════════════════════════════╝

         ████████████████████████████████
         ██ ▄▄▄▄▄ █▀▀▀▀▀▀▀▀▀█ ▄▄▄▄▄ ██
         ██ █   █ █▀ █▀▀█ ▀██ █   █ ██
         ██ ▀▀▀▀▀ █▀ █ █ ▀██ ▀▀▀▀▀ ██
         ████████████████████████████████

    URL:  http://192.168.1.42:7878
    DIR:  D:\Downloads

    >> Press ENTER to launch TUI...
```

---

## ⚡ Features

| Feature | Description |
|---------|-------------|
| 📱 **Phone → Laptop** | Open a URL on your phone's browser — no app needed |
| 🔒 **Zero Cloud** | Files never leave your local network |
| 🎯 **QR Code** | Scan to connect — no typing IP addresses |
| 📦 **Large Files** | Streaming SHA-256 — supports **10GB+ files** without RAM issues |
| 🖥️ **Hacker TUI** | Matrix-green terminal dashboard with real-time progress |
| 🌐 **Web UI** | Built-in mobile-optimized interface with matrix rain animation |
| ⚡ **Fast** | Direct Wi-Fi transfer at full LAN speed |
| 🔐 **mTLS Ready** | Certificate-based device pairing with SHA-256 verification |
| 📡 **mDNS** | Auto-discovery of devices on the network |
| 🦀 **Pure Rust** | Single binary, zero runtime dependencies |

---

## 🚀 Quick Start

### 1. Install

```bash
# Clone and install globally
git clone https://github.com/YaswanthKumarMallela01/FileDrop.git
cd FileDrop
cargo install --path .
```

After installation, `filedrop` is available globally from any directory.

### 2. Receive Files (On Laptop)

```bash
# Navigate to where you want files saved
cd ~/Downloads

# Start the receiver
filedrop receive
```

This shows a **QR code** in your terminal. The QR code stays visible until you press Enter.

### 3. Send Files (From Phone)

1. **Scan the QR code** with your phone camera (or type the URL)
2. A hacker-themed web interface opens in your browser
3. Tap **"> SELECT FILES"** → choose files
4. Tap **"▶ EXECUTE TRANSFER"**
5. Watch real-time progress on both phone and laptop

> **Both devices must be on the same Wi-Fi network.**

---

## 🛠️ All Commands

```
filedrop receive     Start server + TUI, save files to current directory
filedrop send <path> Send file/directory to a discovered peer
filedrop pair        Generate QR code for certificate pairing
filedrop peers       List all paired devices
filedrop unpair <n>  Remove a paired device
filedrop demo        Visual TUI test with simulated transfers
filedrop --help      Show all commands
```

### Examples

```bash
# Receive files into Downloads folder
cd ~/Downloads && filedrop receive

# Receive files into Desktop
cd ~/Desktop && filedrop receive

# Send a file to a discovered peer
filedrop send ./report.pdf

# Send to a specific address
filedrop send ./photos/ --addr 192.168.1.100:7878
```

---

## 🔧 Windows Firewall (One-Time Setup)

Windows blocks incoming connections by default. Open an **Admin PowerShell** and run:

```powershell
netsh advfirewall firewall add rule name="FileDrop" dir=in action=allow protocol=TCP localport=7878 profile=any
```

---

## 🏗️ Architecture

```
                    ┌─────────────────────────────┐
                    │   📱 Phone Browser           │
                    │                               │
                    │  ┌─────────────────────────┐  │
                    │  │  FileDrop Web UI         │  │
                    │  │  • File picker           │  │
                    │  │  • Streaming SHA-256     │  │
                    │  │  • 256KB chunked send    │  │
                    │  │  • Matrix rain ✦         │  │
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
                    │  │  • Write binary chunks   │    │
                    │  │  • Verify SHA-256        │    │
                    │  └───────────┬─────────────┘    │
                    │              │                   │
                    │  ┌───────────▼─────────────┐    │
                    │  │  Ratatui TUI Dashboard   │    │
                    │  │  • File queue            │    │
                    │  │  • Transfer log          │    │
                    │  │  • Speed sparkline       │    │
                    │  └─────────────────────────┘    │
                    └─────────────────────────────────┘
```

### Wire Protocol

```
Phone → Laptop:
  1. JSON:   {"type":"file_start", "name":"photo.jpg", "size":5242880, "sha256":"abc..."}
  2. Binary: [256KB chunk] [256KB chunk] [256KB chunk] ...
  3. JSON:   {"type":"file_done", "checksum":"sha256:abc..."}

Laptop → Phone:
  4. JSON:   {"type":"file_ack", "success":true}
```

---

## 📁 Project Structure

```
FileDrop/
├── Cargo.toml                    Dependencies & build config
├── README.md                     This file
└── src/
    ├── main.rs            ─────  CLI entry point + QR display
    ├── config.rs          ─────  TOML config (~/.config/filedrop/)
    ├── security/
    │   ├── certs.rs       ─────  X.509 certificates (rcgen)
    │   └── pairing.rs     ─────  QR code pairing flow
    ├── transfer/
    │   ├── protocol.rs    ─────  Wire protocol + JSON messages
    │   ├── chunker.rs     ─────  SHA-256 file hashing + chunked I/O
    │   ├── server.rs      ─────  Axum WebSocket server + web routes
    │   └── client.rs      ─────  WebSocket send client
    ├── discovery/
    │   └── mod.rs         ─────  mDNS (Bonjour) advertisement
    ├── tui/
    │   ├── app.rs         ─────  State management + event loop
    │   ├── events.rs      ─────  Keyboard input handler
    │   └── ui.rs          ─────  Ratatui rendering (hacker theme)
    └── web/
        ├── mod.rs         ─────  Embedded HTML server
        └── index.html     ─────  Mobile web UI (hacker theme)
```

---

## 🧰 Tech Stack

| Component | Technology |
|-----------|-----------|
| Language | Rust (2024 edition) |
| Async Runtime | Tokio |
| HTTP Server | Axum |
| WebSocket | tokio-tungstenite |
| TUI Framework | Ratatui + Crossterm |
| TLS | rustls + rcgen |
| Discovery | mdns-sd |
| Hashing | SHA-256 (sha2 crate + pure JS) |
| QR Code | qrcode crate |
| CLI Parser | Clap v4 |

---

## 📄 License

MIT License — use it however you want.

---

<p align="center">
  <sub>Built with 🦀 Rust · No cloud · No trace · Just raw speed</sub>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Version-0.3.2-00e87b?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/HTML-CSS-JS-blue?style=for-the-badge" alt="HTML CSS JS">
  <img src="https://img.shields.io/badge/GitHub%20Pages-Ready-222?style=for-the-badge&logo=github&logoColor=white" alt="GitHub Pages">
  <img src="https://img.shields.io/badge/License-MIT-yellow?style=for-the-badge" alt="License">
</p>

<h1 align="center">
  <code>[ FD-V-RWS ]</code>
  <br>
  <sub>FileDrop — Official Website & Download Portal</sub>
</h1>

<p align="center">
  <b>The official landing page for <a href="https://github.com/YaswanthKumarMallela01/FileDrop">FileDrop</a></b> — a secure, zero-cloud, peer-to-peer file transfer tool.<br>
  Built as a static website, deployable via GitHub Pages.
</p>

---

## 🌐 Live Website

> **[Visit the FileDrop Website →](https://yaswanthkumarmallela01.github.io/FD-V-RWS/)**

---

## ✨ Features

- **Premium Dark Theme** — Glassmorphism cards, 3D transforms, and a deep navy-to-black color palette
- **Animated Particle Background** — Interactive canvas with connected nodes that respond to viewport size
- **Auto OS Detection** — Automatically highlights the correct download card for the visitor's platform (Windows/Linux/macOS)
- **Tabbed Installation Guide** — OS-specific step-by-step instructions with copy-to-clipboard code blocks
- **Comparison Table** — Side-by-side comparison of FileDrop vs Google Drive, Bluetooth, and USB
- **Responsive Design** — Fully optimized for mobile, tablet, and desktop viewports
- **Scroll-Reveal Animations** — Staggered entry animations using Intersection Observer
- **SEO Optimized** — Proper meta tags, Open Graph, semantic HTML5, single `<h1>`
- **Zero Dependencies** — Pure HTML, CSS, and vanilla JavaScript — no frameworks, no build step

---

## 📁 Project Structure

```
FD-V-RWS/
├── index.html              Main page (Hero, Features, Comparison, Download, Instructions, Commands)
├── style.css               Design system (custom properties, glassmorphism, 3D, responsive)
├── script.js               Particles, scroll-reveal, tabs, OS detection, copy-to-clipboard
├── assets/
│   └── images/
│       ├── hero.png        Hero illustration (devices + encrypted tunnel)
│       ├── security.png    Feature icon (shield + lock)
│       ├── speed.png       Feature icon (lightning bolt)
│       └── privacy.png     Feature icon (no-tracking eye)
└── README.md               This file
```

---

## 🚀 Deploying on GitHub Pages

This website is designed to be deployed directly from the `master` (or `main`) branch of this repository via GitHub Pages.

### Steps:

1. **Create a new GitHub repository** named `FD-V-RWS` (or any name you prefer)
2. **Push the code** to the repository:
   ```bash
   cd FD-V-RWS
   git init
   git add .
   git commit -m "Initial commit: FileDrop landing page"
   git remote add origin https://github.com/YaswanthKumarMallela01/FD-V-RWS.git
   git branch -M main
   git push -u origin main
   ```
3. **Enable GitHub Pages:**
   - Go to **Settings** → **Pages**
   - Under **Source**, select **Deploy from a branch**
   - Select the `main` branch and `/ (root)` folder
   - Click **Save**
4. **Your site will be live** at:
   ```
   https://yaswanthkumarmallela01.github.io/FD-V-RWS/
   ```

---

## 🔗 Related

- **[FileDrop](https://github.com/YaswanthKumarMallela01/FileDrop)** — The main FileDrop application (Rust)
- **[Releases](https://github.com/YaswanthKumarMallela01/FileDrop/releases)** — Pre-built binaries for Windows, Linux, and macOS

---

## 🎨 Design Highlights

| Element | Implementation |
|---------|---------------|
| **Background** | Canvas-based particle network with dynamic connections |
| **Cards** | Glassmorphism (`backdrop-filter: blur`) with gradient borders on hover |
| **Typography** | Inter (sans-serif) + JetBrains Mono (code) from Google Fonts |
| **Colors** | Deep navy (`#050a14`) · Accent green (`#00e87b`) · Teal (`#00d4ff`) |
| **Animations** | CSS keyframes + Intersection Observer scroll-reveal with staggered delays |
| **3D Effects** | `perspective` + `rotateX` transforms on hero image and download cards |
| **Responsiveness** | CSS Grid with `repeat(auto-fit)` + flexbox fallbacks for mobile |

---

## 📄 License

MIT License — use it however you want.

---

<p align="center">
  <sub>Built with ❤️ by <a href="https://github.com/YaswanthKumarMallela01">Yaswanth Kumar Mallela</a></sub>
</p>

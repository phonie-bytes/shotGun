# 🎯 shotGun

> High-performance, lightweight screen and region capture utility written in **Rust** with global OS hotkeys, interactive freeze-frame ROI selection, system tray integration, Windows auto-start, configurable encoders, and sequential auto-incrementing naming.
>
> **Created by Noerotech**

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows-0078D6.svg)](https://microsoft.com/windows)
[![Version](https://img.shields.io/badge/version-0.2.0-brightgreen.svg)](Cargo.toml)

---

## 📸 Overview

**shotGun** is designed for ultra-fast, seamless screenshot capturing of specific monitors and regions of interest (ROI). It runs in the background with native global hotkeys so you can take repeated, precisely cropped screenshots during gameplay, video analysis, document scanning, tutorial creation, or UI design without interrupting your workflow.

---

## 🌟 Key Features (v0.2.0)

- 🖥️ **Multi-Monitor Support**: Automatically detects all connected displays, resolutions, positions, scale factors, and primary status.
- ✂️ **Interactive Drag-Select Overlay**: Freeze the screen and drag a bounding box with live pixel dimension badges and coordinates to define your ROI.
- 0️⃣ **Page 0 / 0-Indexed Numbering**: Configure numbering to start at `0` (e.g. `shot_000.png` for cover pages or 0-indexed sequences) or `1`.
- 🔄 **Interactive Session Reset Dialog**: When starting a new session (hotkey `F10`), easily customize the target directory, session name, file prefix, and starting number on the fly.
- 📥 **System Tray Integration**: Minimize shotGun to the Windows system tray with a native context menu (*Show/Hide*, *Capture Region*, *Start New Session*, *Exit*).
- 🚀 **Windows Auto-Start**: Easily toggle automatic startup on Windows boot via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- ⌨️ **Global OS Hotkeys**: Works across any window, fullscreen game, or background app using native Win32 `RegisterHotKey` (zero polling CPU overhead).
- 🔢 **Sequential Filename Incrementing**: Automatically formats filenames with configurable zero-padding (e.g. `shot_001.png`, `shot_002.png`, `shot_003.png`).
- 🖼️ **Multiple Output Formats**:
  - **PNG** (Lossless high-definition)
  - **JPEG** (Configurable 1–100% quality slider)
  - **BMP** (Raw uncompressed bitmap)
  - **WebP** (Modern efficient compression)
- 📋 **History & Clipboard Integration**: View capture history with resolution, file size, timestamp, one-click open, locate in Windows Explorer, or copy image directly to the system clipboard.
- 🔊 **Audio Feedback**: Subtle Windows audio chime on capture (toggleable).
- 📐 **Compact & Clean UI**: Redesigned lightweight window footprint (~580×440 px).
- ⚙️ **Persistent Configuration**: Automatically saves settings to `config.json` between runs.

---

## 🚀 Quick Start

### Prerequisites

- **Rust 1.75+** (`rustup default stable`)
- **Windows 10 / 11**

### Installation & Running

1. **Clone the repository**:
   ```bash
   git clone https://github.com/phonie-bytes/shotGun.git
   cd shotGun
   ```

2. **Run in Development Mode**:
   ```bash
   cargo run
   ```

3. **Build Optimized Release Binary**:
   ```bash
   cargo build --release
   ```
   The executable will be generated at `target/release/shotgun.exe`.

---

## 🎮 How to Use

### 1. Select Screen & Region
- Open the **🖥️ Screen** tab.
- Pick your target monitor from the list.
- Choose **Full Screen** or click **🎯 Drag-Select ROI on Screen** to freeze the display and draw a bounding box.
- Alternatively, use quick presets (*Top 50%*, *Bottom 50%*, *Left 50%*, *Right 50%*, *Center 50%*) or type manual pixel coordinates.

### 2. Configure Hotkeys
- Open the **⌨️ Hotkeys** tab.
- Set your **Capture Hotkey** (Default: `F9` or any combination like `Ctrl + Shift + S`).
- Set your **New Session / Reset Hotkey** (Default: `F10` or `Ctrl + Shift + R`).
- Click **💾 Save & Apply Hotkey Changes**.

### 3. Output Destination & Page 0 Numbering
- Open the **📁 Output** tab.
- Choose your destination folder.
- Select starting number: **0 (Page 0)** or **1 (Page 1)**.
- Choose file format (**PNG**, **JPEG**, **BMP**, **WebP**).
- Enable/disable **Prompt on New Session**.

### 4. Background Running & System Tray
- Open **⚙️ Settings** to enable **Launch on Windows startup** or **Keep running in System Tray when minimized**.
- Close or minimize shotGun to hide it to the system tray. Double-click the tray icon or use the context menu to restore it.

---

## 🎥 Video + Desktop Audio Recording Architecture (Technical Roadmap)

```
┌──────────────────────────────────────────────────────────┐
│                   shotGun Video Pipeline                 │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  [Screen / ROI]  ──────> DXGI / WGC ─────> NV12 Frames   │
│                                                 │        │
│  [Desktop Audio] ──────> WASAPI Loopback ─> PCM Audio    │
│                                                 │        │
│                                                 ▼        │
│                             Media Foundation / FFmpeg    │
│                                    (H.264 + AAC)         │
│                                                 │        │
│                                                 ▼        │
│                                            Output .mp4   │
└──────────────────────────────────────────────────────────┘
```

1. **Continuous ROI Frame Extraction**:
   - Uses **Windows Graphics Capture (WGC)** or **DXGI Desktop Duplication** for GPU-accelerated direct memory frame grabs at 60 FPS.
   - Crops directly to the selected ROI coordinates $(X, Y, W, H)$ in hardware before encoding.
2. **Desktop Audio Capture (WASAPI Loopback)**:
   - Uses Windows Core Audio **WASAPI in Loopback Mode** (`AUDCLNT_STREAMFLAGS_LOOPBACK`) to capture all desktop sound (e.g. playing videos, game audio, meeting audio) directly from the default playback endpoint.
3. **Muxing to MP4**:
   - Hardware-accelerated H.264 video encoding + AAC audio encoding via Windows Media Foundation (`mfplat`) or bundled encoder into standard `.mp4` containers.

---

## 🛠️ Project Structure

```
shotGun
├── src/
│   ├── main.rs       # Application entry point & eframe window setup
│   ├── app.rs        # egui UI layout, tabs, modal dialogs, and history
│   ├── autostart.rs  # Windows Startup registry management (HKCU/.../Run)
│   ├── capture.rs    # Screen capture engine (xcap), ROI cropping & image encoders
│   ├── config.rs     # Configuration model, Serde serialization & defaults
│   ├── hotkey.rs     # Dedicated Win32 RegisterHotKey message loop thread & channels
│   ├── overlay.rs    # Interactive fullscreen freeze-frame region selection tool
│   └── tray.rs       # System tray icon with context menu & event dispatcher
├── Cargo.toml        # Dependencies & package metadata (Noerotech)
├── README.md         # Comprehensive user & developer guide
├── TODO.md           # Roadmap & completed milestones
├── BUGS.md           # Known issues, DPI edge cases & mitigation log
└── LESSONS.md        # Engineering insights & architecture takeaways
```

---

## 🧪 Testing

Run automated unit tests:
```bash
cargo test
```

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.  
Created by **Noerotech** (2026).

# 🎯 shotGun

> High-performance, lightweight screen and region capture utility written in **Rust** with global OS hotkeys, interactive freeze-frame ROI selection, configurable encoders, and sequential auto-incrementing naming.

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows-0078D6.svg)](https://microsoft.com/windows)

---

## 📸 Overview

**shotGun** is designed for ultra-fast, seamless screenshot capturing of specific monitors and regions of interest (ROI). It runs in the background with native global hotkeys so you can take repeated, precisely cropped screenshots during gameplay, video analysis, document scanning, tutorial creation, or UI design without interrupting your workflow.

### 🌟 Key Features

- 🖥️ **Multi-Monitor Support**: Automatically detects all connected displays, resolutions, positions, scale factors, and primary status.
- ✂️ **Interactive Drag-Select Overlay**: Freeze the screen and drag a bounding box with live pixel dimension badges and coordinates to define your ROI.
- ⌨️ **Global OS Hotkeys**: Works across any window, fullscreen game, or background app using native Win32 `RegisterHotKey` (zero polling CPU overhead).
- 🔢 **Sequential Filename Incrementing**: Automatically formats filenames with configurable zero-padding (e.g. `shot_001.png`, `shot_002.png`, `shot_003.png`).
- 🔄 **"Start New Session" Hotkey / Button**: Instantly reset the numbering counter to 1 or advance to a new session directory (e.g. `captures/session_02/`).
- 🖼️ **Multiple Output Formats**:
  - **PNG** (Lossless high-definition)
  - **JPEG** (Configurable 1–100% quality slider)
  - **BMP** (Raw uncompressed bitmap)
  - **WebP** (Modern efficient compression)
- 📋 **History & Clipboard Integration**: View capture history with resolution, file size, timestamp, one-click open, locate in Windows Explorer, or copy image directly to the system clipboard.
- 🔊 **Audio Feedback**: Subtle Windows audio chime on capture (toggleable).
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
- Open the **🖥️ Screen & Region** tab.
- Pick your target monitor from the list.
- Choose **Full Screen** or click **🎯 Interactive Drag-Select on Screen** to drag a bounding box.
- Alternatively, use quick presets (*Top Half*, *Bottom Half*, *Left 50%*, *Right 50%*, *Center 50%*) or type manual pixel coordinates.

### 2. Configure Hotkeys
- Open the **⌨️ Global Hotkeys** tab.
- Set your **Capture Hotkey** (Default: `F9` or any combination like `Ctrl + Shift + S`).
- Set your **New Session / Reset Hotkey** (Default: `F10` or `Ctrl + Shift + R`).
- Click **💾 Save & Apply Hotkey Changes**. The status indicator in the top bar will turn green (`🟢 Hotkeys Active`).

### 3. Output Destination & Format
- Open the **📁 Output & Naming** tab.
- Choose your destination folder (Defaults to your user `Pictures/shotGun` directory).
- Choose your preferred file format (**PNG**, **JPEG**, **BMP**, **WebP**).
- Customize filename prefix (`shot_`) and zero-padding digits (`3` -> `001`).

### 4. Capture!
- Press your capture hotkey (e.g. `F9`) from any app or game.
- The screenshot will be saved immediately with an incremented number.
- When starting a new sequence or topic, press the new session hotkey (e.g. `F10`) to reset numbering back to 1.

---

## 🛠️ Architecture

```
shotGun
├── src/
│   ├── main.rs      # Application entry point & eframe window setup
│   ├── app.rs       # egui UI layout, tabs, history, and user interactions
│   ├── capture.rs   # Screen capture engine (xcap), ROI cropping & image encoders
│   ├── config.rs    # Configuration model, Serde serialization & defaults
│   ├── hotkey.rs    # Dedicated Win32 RegisterHotKey message loop thread & channels
│   └── overlay.rs   # Interactive fullscreen freeze-frame region selection tool
├── Cargo.toml       # Dependencies & package metadata
├── README.md        # Comprehensive user & developer guide
├── TODO.md          # Roadmap & planned features
├── BUGS.md          # Known issues, DPI edge cases & mitigation log
└── LESSONS.md       # Engineering insights, architecture decisions & takeaways
```

### Threading Model

- **Main UI Thread**: Runs the `egui` immediate-mode event loop at 60 FPS (or on demand), managing UI controls, live previews, and image displays.
- **Hotkey Listener Thread**: Dedicated background thread running a native Windows `PeekMessageW` loop registered with `RegisterHotKey`. Uses lock-free `crossbeam-channel` to notify the UI thread with zero jitter or latency.

---

## 🧪 Testing

Run automated unit tests:
```bash
cargo test
```

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

# 🎯 shotGun

> High-performance, lightweight screen and region capture utility written in **Rust** with global OS hotkeys, interactive freeze-frame ROI selection, system tray integration, Windows auto-start, configurable encoders, and sequential auto-incrementing naming.
>
> **Created by Noerotech**

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows-0078D6.svg)](https://microsoft.com/windows)
[![Version](https://img.shields.io/badge/version-0.4.0-brightgreen.svg)](Cargo.toml)

---

## 📸 Overview

**shotGun** is designed for ultra-fast, seamless screenshot capturing of specific monitors and regions of interest (ROI). It runs in the background with native global hotkeys so you can take repeated, precisely cropped screenshots during gameplay, video analysis, document scanning, tutorial creation, or UI design without interrupting your workflow.

---

## 🌟 Key Features (v0.4.0)

- 🗂️ **Capture Profiles**: Named, switchable capture setups — e.g. a "Teams" profile with its own region/output folder/filename prefix, and a separate "Video Recording" profile with its own monitor/fps/output — switch between them with one click instead of re-pointing settings every time.
- 🖌️ **Post-Capture Annotation Editor**: After a Quick Region Capture, annotate directly on the frozen overlay before saving — Arrow, Rectangle, Highlighter, Text, Blur/Pixelate (adjustable strength), Redact (solid opaque), and numbered Step Labels, with a color picker and full undo.

- 🖥️ **Multi-Monitor Support**: Automatically detects all connected displays, resolutions, positions, scale factors, and primary status.
- ✂️ **Interactive Drag-Select Overlay**: Freeze the screen and drag a bounding box with live pixel dimension badges and coordinates to define your ROI.
- 0️⃣ **Page 0 / 0-Indexed Numbering**: Configure numbering to start at `0` (e.g. `shot_000.png` for cover pages or 0-indexed sequences) or `1`.
- 🔄 **Interactive Session Reset Dialog**: When starting a new session (hotkey `F10`), easily customize the target directory, session name, file prefix, and starting number on the fly.
- 🎥 **Video Recording with Audio**: Record the selected ROI to MP4 via DXGI Desktop Duplication, with system audio muxed in via WASAPI loopback capture. See [Video Recording](#-video-recording) below.
- 📄 **Multi-Screenshot PDF Export**: Bundle a session's screenshots into a single PDF — export the current History tab manually at any time, or auto-export whenever a session ends (opt-in).
- 📥 **System Tray Integration**: Minimize shotGun to the Windows system tray with a native context menu (*Show/Hide*, *Capture Region*, *Start New Session*, *Exit*).
- 🚀 **Windows Auto-Start**: Easily toggle automatic startup on Windows boot via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- ⌨️ **Global OS Hotkeys**: Works across any window, fullscreen game, or background app using native Win32 `RegisterHotKey` (zero polling CPU overhead) — Capture, New Session, Video Start/Stop, and Show/Hide Window (a guaranteed way back in if the window ever gets hidden).
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
- If the window ever gets hidden and the tray icon isn't reachable, press the Show/Hide Window hotkey (default `Ctrl+Alt+Insert`, configurable on the Hotkeys tab).

### 5. Record Video
- Click **⏺ Record Video** in the header (or use the Video Start/Stop hotkeys, default `F12` / `Shift+F12`).
- Click **⏹ Stop Video** when done; the status bar reports the result once encoding finishes.
- Configure **Target FPS** and an optional **ffmpeg Path** (auto-detected from the app folder or PATH otherwise) on the Settings tab, along with an option to delete the intermediate PNG frames/audio.wav once the MP4 is successfully created.

### 6. Export Screenshots to PDF
- On the **📜 History** tab, click **📄 Export to PDF** to bundle everything currently listed there into one PDF (a save dialog lets you pick the destination).
- Alternatively, enable **Automatically bundle a session's screenshots into a PDF when starting a New Session** on the Settings tab — it exports each session's shots into that session's folder as the session ends. Individual image files are always kept either way; the PDF is purely an additional output.

---

## 🎥 Video Recording

Click **⏺ Record Video** in the header (or press the Video Start/Stop hotkeys, default `F12` / `Shift+F12`) to record the currently selected ROI to MP4, with system audio muxed in automatically if something is playing.

```
┌────────────────────────────────────────────────────────────────┐
│                     shotGun Video Pipeline                     │
├────────────────────────────────────────────────────────────────┤
│                                                                 │
│  [Screen / ROI] ──> DXGI Desktop Duplication ──> PNG frames     │
│        (src/dxgi_capture.rs)      (async writer pool, N cores) │
│                                          │                      │
│  [Desktop Audio] ──> WASAPI Loopback ──> audio.wav              │
│        (src/audio.rs)                   │                      │
│                                          ▼                      │
│                    ffmpeg (concat demuxer, per-frame            │
│                    real duration + libx264/AAC, no console)     │
│                                          │                      │
│                                          ▼                      │
│                                    Output .mp4                  │
└────────────────────────────────────────────────────────────────┘
```

1. **Frame capture** (`src/dxgi_capture.rs`): talks to DXGI Desktop Duplication directly rather than through a third-party wrapper, because getting this right matters — it correctly enumerates *every* graphics adapter's outputs to find whichever one actually owns the target monitor (needed for multi-GPU/hybrid-graphics laptops, where a naive "default adapter" lookup fails outright on non-primary monitors). A duplication session is created once per monitor and reused across recordings, since DXGI only permits one active session per output at a time.
2. **Async encoding**: PNG-encoding a large frame can take tens of milliseconds — done synchronously inline, that alone throttles capture to the encoder's speed regardless of how fast the screen is actually changing. Captured frames are instead handed off to a small pool of dedicated encoder threads (sized to CPU core count) so DXGI polling is never blocked on disk/CPU encode work.
3. **Real per-frame timing**: DXGI only delivers a frame when the screen actually changes, so capture is inherently bursty. Rather than averaging `frame_count ÷ elapsed_time` into one `-framerate` (which would stretch fast bursts into slow motion and compress quiet stretches), each frame's true real-world display duration is tracked and handed to ffmpeg via a concat-demuxer script, then resampled to a standard constant frame rate (`-r`, default 30fps) for normal playback everywhere.
4. **Audio** (`src/audio.rs`): captures system audio loopback via WASAPI directly (`cpal` doesn't expose loopback mode), muxed in only when real samples were actually captured — silent recordings correctly degrade to video-only instead of corrupting the whole output.
5. **Muxing**: shells out to `ffmpeg` (bundled `ffmpeg.exe` next to the app, a configured path, or PATH — configurable in Settings), running hidden with no console window. Optional cleanup of intermediate PNGs/WAV after a successful encode is available in Settings.

---

## 🛠️ Project Structure

```
shotGun
├── src/
│   ├── main.rs         # Application entry point & eframe window setup
│   ├── app.rs          # egui UI layout, tabs, modal dialogs, and history
│   ├── audio.rs        # WASAPI loopback system-audio capture to WAV
│   ├── autostart.rs    # Windows Startup registry management (HKCU/.../Run)
│   ├── capture.rs      # Screenshot capture engine (xcap), ROI cropping & image encoders
│   ├── config.rs       # Configuration model, Serde serialization & defaults
│   ├── dxgi_capture.rs # Direct DXGI Desktop Duplication (multi-adapter aware)
│   ├── hotkey.rs       # Dedicated Win32 RegisterHotKey message loop thread & channels
│   ├── overlay.rs      # Interactive fullscreen freeze-frame region selection tool
│   ├── pdf_export.rs   # Combines captured screenshots into a PDF (printpdf)
│   ├── tray.rs         # System tray icon with context menu & event dispatcher
│   └── video.rs        # Video recording orchestration: capture + audio + ffmpeg mux
├── Cargo.toml          # Dependencies & package metadata (Noerotech)
├── README.md           # Comprehensive user & developer guide
├── TODO.md             # Roadmap & completed milestones
├── BUGS.md             # Known issues, DPI edge cases & mitigation log
└── LESSONS.md          # Engineering insights & architecture takeaways
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

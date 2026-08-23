# 💡 shotGun - Engineering Lessons & Architecture Insights

This document captures key engineering decisions, architectural trade-offs, and technical lessons learned during the design and implementation of **shotGun**.

---

## 1. Global Hotkeys: Win32 `RegisterHotKey` vs Low-Level Keyboard Hooks (`WH_KEYBOARD_LL`)

### Context
We needed a global hotkey system that intercepts keystrokes across the entire OS (when the app is minimized or unfocused) without adding latency or lagging gameplay.

### Decision
We implemented native Win32 `RegisterHotKey` in a dedicated message pump thread (`PeekMessageW` loop).

### Key Takeaways
- **Performance**: `RegisterHotKey` is managed in the kernel's window manager. It has zero CPU overhead while idle and does not hook into every key event, unlike `SetWindowsHookExW` (`WH_KEYBOARD_LL`) which processes every single keystroke system-wide.
- **Reliability & Anti-Cheat**: Many multiplayer games block or flag low-level keyboard hooks as suspicious macro/cheat software. `RegisterHotKey` is standard Windows API and is treated normally.
- **Thread Affinity**: `RegisterHotKey` binds hotkeys to the specific thread that registered them. Running the registration and message loop on the same dedicated background thread avoids synchronization deadlocks.

---

## 2. Multi-Monitor Screen Coordinate Spaces & DPI Scaling

### Context
Modern setups frequently mix 4K screens (150% DPI scale) with 1080p screens (100% scale), where monitors can be positioned with negative virtual coordinates.

### Decision
- Decouple **Monitor Capture Coordinates** from **Virtual Desktop Coordinates**.
- Perform cropping strictly in device physical pixels `(0, 0, physical_width, physical_height)`.
- Use the egui viewport's `screen_rect` to compute coordinate transformation ratios:
  ```rust
  let scale_x = monitor_physical_width as f32 / egui_screen_rect.width();
  let scale_y = monitor_physical_height as f32 / egui_screen_rect.height();
  ```

### Key Takeaways
- Never assume 1 logical point equals 1 physical pixel.
- Freezing the screen into a memory buffer before opening the snipping overlay ensures that fast-moving content (videos, animations) doesn't shift or blur while the user is drawing the region bounding box.

---

## 3. High-Throughput Image Encoding and Memory Management

### Context
Taking bursts of screenshots at high resolutions (4K / 1440p) requires efficient memory allocation and fast disk I/O.

### Decision
- Used `image::imageops::crop_imm` which avoids duplicating the full image buffer before cropping.
- Wrapped file output streams in `BufWriter` for buffered disk I/O.
- Handled color space conversions properly (e.g. converting 4-channel `RgbaImage` to 3-channel `Rgb8` when encoding JPEG).

---

## 4. UI State Management with `eframe` / `egui`

### Context
`egui` is an immediate-mode GUI library. In immediate mode, UI elements do not hold long-term state across frames unless explicitly stored in application structs.

### Key Takeaways
- Keep transient state (such as combo box selected indices and drag bounding boxes) in the app state.
- Use `ctx.request_repaint_after(Duration::from_millis(50))` to keep UI responsive to background hotkey events without maxing out CPU frames when idle.
- Channel communication via `crossbeam-channel` (`try_recv()`) cleanly bridges asynchronous background threads with egui's single-threaded render loop.

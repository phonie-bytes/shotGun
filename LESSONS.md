# 💡 shotGun - Engineering Lessons & Architecture Insights

This document captures key engineering decisions, architectural trade-offs, and technical lessons learned during the design and implementation of **shotGun v0.2.0** by **Noerotech**.

---

## 1. System Tray Management in Immediate-Mode GUIs (`egui`)

### Context
Integrating a persistent background system tray icon with an immediate-mode GUI framework like `egui` / `eframe` requires coordinating window visibility, message queues, and cross-thread event dispatching.

### Key Takeaways
- Immediate-mode GUIs reconstruct the interface every frame. We decouple the tray handler into a persistent `TrayHandler` holding event receivers (`MenuEvent`, `TrayIconEvent`).
- Using `ctx.send_viewport_cmd(ViewportCommand::Visible(bool))` and `ViewportCommand::Focus` allows toggling the window smoothly without destroying the renderer state or interrupting ongoing background hotkey listeners.

---

## 2. Windows Startup Registry Integration via `winreg`

### Context
We needed a seamless way for users to enable/disable auto-start on Windows boot directly within the application without installer scripts or admin permissions.

### Key Takeaways
- Registering into `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` avoids elevation/UAC requirements.
- Storing the exact canonical executable path (`std::env::current_exe()`) wrapped in quotes ensures reliable execution even if the folder path contains spaces.

---

## 3. Global Hotkeys: Win32 `RegisterHotKey` vs Low-Level Keyboard Hooks (`WH_KEYBOARD_LL`)

### Key Takeaways
- **Performance**: `RegisterHotKey` is managed in the kernel's window manager. It has zero CPU overhead while idle and does not hook into every key event, unlike `SetWindowsHookExW` (`WH_KEYBOARD_LL`) which processes every single keystroke system-wide.
- **Reliability & Anti-Cheat**: Many multiplayer games block or flag low-level keyboard hooks as suspicious macro/cheat software. `RegisterHotKey` is standard Windows API and is treated normally.
- **Thread Affinity**: `RegisterHotKey` binds hotkeys to the specific thread that registered them. Running the registration and message loop on the same dedicated background thread avoids synchronization deadlocks.

---

## 4. Multi-Monitor Coordinate Spaces & Physical DPI Normalization

### Key Takeaways
- Never assume 1 logical point equals 1 physical pixel.
- Freezing the screen into a memory buffer before opening the snipping overlay ensures that fast-moving content (videos, animations) doesn't shift or blur while the user is drawing the region bounding box.
- Local coordinate clamping prevents out-of-bounds panics when dragging beyond the monitor boundary.

---

## 5. DXGI Desktop Duplication: One Session Per Output, and Trust No Wrapper's Adapter Selection

### Context
Building video recording surfaced two non-obvious DXGI constraints the hard way, both from an initial implementation built on a third-party crate's `video_recorder()` helper.

### Key Takeaways
- **DXGI only permits one active duplication interface per monitor output at a time system-wide.** A naive "create a fresh duplication session per recording" design works exactly once, then silently fails to re-acquire it on every subsequent recording in the same process. The fix: create the session once per monitor (lazily, on first use) and reuse it across recordings via a pause/resume signal (`Mutex<bool> + Condvar`), never recreating it.
- **Never assume `D3D11CreateDevice(None, ...)`'s "default adapter" owns the monitor you want.** On any multi-GPU/hybrid-graphics system, it doesn't necessarily. The correct approach is always: enumerate every adapter (`IDXGIFactory1::EnumAdapters1`), enumerate every output on each (`IDXGIAdapter1::EnumOutputs`), match by `HMONITOR` (from `MonitorFromPoint`), and only *then* create the D3D11 device against that specific adapter (`D3D_DRIVER_TYPE_UNKNOWN` is required whenever an explicit adapter is passed).
- When copying a GPU texture back to the CPU (`Map`/staging texture), the mapped row pitch is not guaranteed to equal `width * bytes_per_pixel` — GPUs commonly pad each row to an alignment boundary. Failing to strip that padding row-by-row silently corrupts image data on any width that isn't already aligned.

---

## 6. Decouple Slow Per-Frame Work From the Capture Loop, and Measure Before Optimizing

### Context
An early video-recording implementation captured frames correctly but felt like "a slideshow of screenshots, not real video." The instinct was to tune throttling values; the real fix required actually measuring where the time was going.

### Key Takeaways
- **Benchmark before guessing.** PNG-encoding a large frame turned out to take ~3 seconds in a debug build and ~50-100ms in release — a 30-60x difference. Encoding synchronously inside the same loop that polls for new frames meant capture rate was gated by encoder speed, not by how fast the screen was actually changing, regardless of any throttle setting.
- **Decouple acquisition from encoding.** Moving PNG encoding onto a small pool of dedicated worker threads (sized to CPU core count), fed via a channel, let frame *acquisition* proceed at full speed while encoding happened in parallel — the fix that actually mattered, more than any encoder-setting tweak.
- **Don't average timing away.** Screen-change-driven capture is inherently bursty. Computing a single `frame_count / elapsed_time` "average fps" and applying it uniformly stretches fast bursts into slow motion and compresses quiet stretches — it looks wrong even when the math is technically consistent with total duration. Recording each frame's *actual* real-world display duration and feeding it to the encoder via a concat/duration script (then resampling to a standard constant output fps) preserves what actually happened.

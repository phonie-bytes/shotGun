# 🐛 shotGun - Known Bugs, Edge Cases & Troubleshooting

This document tracks known issues, platform-specific edge cases, and their resolutions in **shotGun**.

---

## 🔍 Known Edge Cases & Handling

### 1. Mixed High-DPI Multi-Monitor Setups
- **Symptom**: If Monitor 1 is 4K with 150% DPI scaling and Monitor 2 is 1080p with 100% DPI scaling, screenshot coordinates could misalign.
- **How shotGun Handles This**:
  - `xcap` captures the raw physical pixel buffer directly from the display driver.
  - The region selector overlay calculates the scaling ratio between `screen_rect` in logical egui points and `monitor_width / monitor_height` in raw device pixels, ensuring exact bounding box alignment regardless of Windows display scaling settings.

### 2. Windows Startup Registry Permissions
- **Symptom**: Writing to system-wide `HKLM` requires elevated administrator privileges, triggering UAC prompts or silent failures.
- **How shotGun Handles This**:
  - shotGun registers strictly to the current user registry hive: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\shotGun`.
  - This operates with standard user permissions and boots cleanly without UAC prompts.

### 3. Hotkey Conflicts with Other Applications
- **Symptom**: When registering a hotkey already claimed by another app (e.g. `PrintScreen` by Windows Snipping Tool), `RegisterHotKey` returns error code 0.
- **How shotGun Handles This**:
  - The background thread detects registration failures immediately and dispatches an error status event.
  - The top bar UI turns red (`🔴 Failed to bind Capture - hotkey conflict`) and alerts the user to choose another modifier/key combination.

### 4. Negative Virtual Screen Coordinates in Multi-Monitor Layouts
- **Symptom**: Secondary monitors placed to the left or above the primary monitor have negative `(x, y)` coordinates.
- **How shotGun Handles This**:
  - `xcap` captures monitor textures in their local coordinate frame `(0..width, 0..height)`.
  - Region offsets are kept relative to the selected monitor's origin rather than the virtual desktop origin, preventing negative bounding box exceptions.

### 5. Video Recording Fails on Multi-GPU / Hybrid-Graphics Systems
- **Symptom**: Recording from a specific monitor fails with `DXGI_ERROR_NOT_FOUND (0x887A0002)` — "The object was not found. If calling IDXGIFactory::EnumAdaptes, there is no adapter with the specified ordinal."
- **Cause**: The third-party `xcap` crate's `video_recorder()` creates its D3D11 device against whatever adapter `D3D11CreateDevice(None, ...)` picks as "default," then only searches *that* adapter's outputs for the target monitor. On laptops with integrated + discrete GPUs, or multi-adapter desktops, a monitor attached to the non-default adapter is never found.
- **How shotGun Handles This**: `src/dxgi_capture.rs` bypasses `xcap`'s video capture path entirely and does the adapter/output search correctly — enumerates every `IDXGIAdapter1` via `IDXGIFactory1`, and every output on each, to find whichever adapter actually owns the target monitor (matched via `MonitorFromPoint`), then creates the D3D11 device against that specific adapter.

### 6. Video Recording Produces a 0-Byte MP4 for Hand-Dragged ROIs
- **Symptom**: Recording completes, frames are captured, but the MP4 is 0 bytes; ffmpeg logs `width not divisible by 2`.
- **Cause**: `libx264` with `yuv420p` (4:2:0 chroma subsampling) requires even width *and* height, but a manually drag-selected ROI is frequently an odd pixel size.
- **How shotGun Handles This**: the ffmpeg command always includes `-vf crop=floor(iw/2)*2:floor(ih/2)*2`, silently cropping at most 1px per side to the nearest even dimensions.

### 7. Video Recording Fails Entirely When Nothing Was Playing
- **Symptom**: Recording a silent session (nothing playing) produces "Conversion failed" / a 0-byte MP4 even though video frames were captured fine and the status log claimed audio was "recorded."
- **Cause**: WASAPI's mix format is `WAVE_FORMAT_EXTENSIBLE`, whose header alone is ~68 bytes even with zero recorded samples. A file-size heuristic (`> 44 bytes`) for "does this WAV have real audio" was fooled by that header, so an empty WAV got passed to ffmpeg with `-shortest`, and the zero-duration audio stream collapsed the *entire* output — including the perfectly good video.
- **How shotGun Handles This**: the actual sample count is parsed via `hound::WavReader` before deciding whether to include the audio input at all; a silent recording now correctly falls back to a video-only MP4.

---

## 🛠️ Troubleshooting Guide

| Issue | Cause | Solution |
|---|---|---|
| **Hotkeys not firing when in a game** | The game is running in exclusive fullscreen mode with elevated (Administrator) privileges. | Run shotGun as Administrator so Windows allows hotkey interception over elevated game windows. |
| **Black screen capture on protected DRM video** | Protected media playback (e.g. Netflix in Edge) uses hardware DRM overlay. | Disable hardware acceleration in the browser or capture unprotected content. |
| **App hidden after closing** | "Keep running in System Tray when minimized" is enabled. | Double-click the shotGun tray icon near the Windows clock or right-click to select "Show / Hide shotGun". If the tray icon isn't reachable (collapsed into the overflow chevron), use the Show/Hide Window hotkey (default `Ctrl+Alt+Insert`) instead. |
| **"Keep running in System Tray when minimized" has no effect** | Known limitation (tracked in TODO.md): the setting is currently a no-op — nothing intercepts the native minimize/close event yet. Minimizing just does the normal Windows behavior. | Use the tray icon or Show/Hide Window hotkey manually for now. |
| **Recorded video has no audio despite something playing** | WASAPI loopback only delivers packets while the audio engine is actively rendering; a silent stretch mid-recording may not get continuous padding. | Known limitation (tracked in TODO.md) — not yet a dummy-render-stream workaround. Video-only fallback is otherwise automatic and correct when nothing was playing at all. |

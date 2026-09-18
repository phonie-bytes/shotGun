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

### 8. Window "Disappears" and Show/Hide/Tray/Capture/Exit All Stop Working
- **Symptom**: After hiding the window once (via the Show/Hide hotkey or the tray "Show/Hide" menu item), the app appeared to fully lock up: pressing Show/Hide again did nothing, and neither Capture nor Exit worked from the tray menu either. The process was still running and "responding" in Task Manager, and the tray icon was still there — it just stopped reacting to anything.
- **Cause**: `toggle_window_visibility()` used `ViewportCommand::Visible(false)` to hide the window. On Windows, winit/eframe never delivers `RedrawRequested` to a window with `WS_VISIBLE = false`, and shotGun's `update()` — which is the only place hotkey events, tray menu events, and capture requests actually get drained from their channels — only runs in response to a repaint. So the very first hide made the window unable to ever schedule another repaint, permanently starving `update()` and freezing all of those channels, including the toggle meant to bring it back.
- **How shotGun Handles This**: hiding the window is now implemented purely with `ViewportCommand::Minimized(true)`/`Minimized(false)`, never `Visible(false)`. Minimized (iconic) windows keep receiving `RedrawRequested` on a throttled schedule (eframe just adds a short CPU-saving sleep), so `update()` keeps running and the next Show/Hide, Capture, or Exit action is always processed. Verified via simulated hotkey presses: hide leaves `IsIconic=True` with the process still responding, and a second toggle restores `IsIconic=False`.

### 9. Drag-Select ROI Confined to the Small App Window
- **Symptom**: "🎯 Drag-Select ROI on Screen" froze the screen and let you drag a selection, but the draggable canvas was only as big as the app's own (~580x440) window, making precise selection on a large or high-res monitor awkward.
- **Cause**: the overlay is drawn via an `egui::Area` sized to `ctx.screen_rect()`, which is just the app's own current viewport — nothing ever resized that viewport to match the target monitor, so a full monitor's worth of freeze-frame image and drag coordinates were being squeezed into whatever small canvas the window happened to be.
- **How shotGun Handles This**: `start_drag_select_overlay()` now temporarily removes window decorations and moves/resizes the app window to exactly cover the target monitor's real screen coordinates (`mon.x/y/width/height`, adjusted for `scale_factor`) before starting the freeze-frame capture, so dragging happens 1:1 against real screen pixels. Two implementation details mattered:
  - The move/resize is deferred to the *frame after* `Decorations(false)` is sent (`pending_fullscreen_overlay`), not sent in the same batch — Windows/winit can recreate the native window when decorations toggle, and a position/size command issued in that same batch is prone to being silently dropped mid-recreation.
  - Saving/restoring the window afterward uses the viewport's *inner* size (`ViewportInfo::inner_rect`), not outer size (`outer_rect`) — since restoring re-enables decorations and `ViewportCommand::InnerSize` is content size excluding chrome, saving outer size and reapplying it as inner size would add the title-bar/border thickness on top of itself every time the overlay is used, growing the window a little each time.
  - Verified directly against the real window handle (found via `EnumWindows` + title match — `Process.MainWindowHandle` in a debug build can resolve to the wrong window, e.g. the console window debug builds allocate) on a multi-monitor setup: the window's rect exactly matched the target monitor's real screen coordinates while the overlay was active, a simulated drag produced a pixel-accurate selection box, and the window returned to its exact original size/position afterward.

### 10. Drag-Select Overlay Misplaced/Wrong Size on a Mixed-DPI Monitor
- **Symptom**: selecting a monitor with a different DPI scale than the one the app window started on (e.g. a 150%-scaled middle monitor between two 100%-scaled ones) for "Drag-Select ROI on Screen" only covered part of that monitor and overlapped onto the neighboring monitor to the left; after cancelling, the app window's own size came back wrong ("wonky").
- **Cause**: the target position/size was computed once, upfront, as `monitor_pixels / monitor_scale_factor`, then handed to `ViewportCommand::OuterPosition`/`InnerSize` in the same batch as the move. Those commands are in *points*, which winit converts to physical pixels using whatever DPI context is active *at the moment the command is processed* — while the window is still on the *origin* monitor, not yet the destination. On a same-DPI setup this happens to cancel out; crossing into a differently-scaled monitor it doesn't, and the window ends up sized/positioned for the wrong scale factor. The restore path had the identical issue moving back.
- **First attempt**: computed the destination's physical-pixel bounds as points using the *destination* monitor's own reported scale factor, on the assumption that's what would end up applying. It isn't — `OuterPosition`/`InnerSize` are in points, and winit converts them to physical pixels using whatever DPI context is active *at the moment the command is sent*, which is still the *origin* monitor's right up until the window actually crosses over. Whenever origin and destination had different scale factors, this produced a window sized/positioned for the wrong scale — visibly, a window stretched across multiple monitors starting from the cursor's one (confirmed via the Quick Region Capture hotkey, moving between monitors with different scale).
- **Second attempt**: switched to always converting with whatever scale factor is *currently* active rather than guessing the destination's, re-reading it fresh at each step. Better in principle, but `OuterPosition` and `InnerSize` were still sent in the same batch: on Windows, applying the position move can itself flip the active DPI context *before* the already-queued `InnerSize` (computed against the *old* scale) gets applied, so the size command ends up reinterpreted under the *new* scale anyway — reproducing the same class of bug (observed as an exact 1.5x oversized window on a 150% monitor) despite the "correct" per-step math.
- **How shotGun Handles This**: stopped computing the destination size by hand entirely. Entering now only *positions* the window (roughly) onto the destination monitor using the current scale, then asks the OS for real borderless fullscreen (`ViewportCommand::Fullscreen(true)`) — Windows resolves the exact size against whatever monitor the window ends up on and its real scale factor, natively, so there's no points/pixels conversion left for shotGun to get wrong. Restoring reverses this (`Fullscreen(false)`, then reposition, then — one settled frame later — restore the saved *physical-pixel* size converted to points using whatever scale is active by then), which sidesteps the same race since position and size are applied a frame apart rather than in the same batch.

---

## 🛠️ Troubleshooting Guide

| Issue | Cause | Solution |
|---|---|---|
| **Hotkeys not firing when in a game** | The game is running in exclusive fullscreen mode with elevated (Administrator) privileges. | Run shotGun as Administrator so Windows allows hotkey interception over elevated game windows. |
| **Black screen capture on protected DRM video** | Protected media playback (e.g. Netflix in Edge) uses hardware DRM overlay. | Disable hardware acceleration in the browser or capture unprotected content. |
| **App hidden after closing** | "Keep running in System Tray when minimized" is enabled. | Double-click the shotGun tray icon near the Windows clock or right-click to select "Show / Hide shotGun". If the tray icon isn't reachable (collapsed into the overflow chevron), use the Show/Hide Window hotkey (default `Ctrl+Alt+Insert`) instead. |
| **"Keep running in System Tray when minimized" has no effect** | Known limitation (tracked in TODO.md): the setting is currently a no-op — nothing intercepts the native minimize/close event yet. Minimizing just does the normal Windows behavior. | Use the tray icon or Show/Hide Window hotkey manually for now. |
| **Recorded video has no audio despite something playing** | WASAPI loopback only delivers packets while the audio engine is actively rendering; a silent stretch mid-recording may not get continuous padding. | Known limitation (tracked in TODO.md) — not yet a dummy-render-stream workaround. Video-only fallback is otherwise automatic and correct when nothing was playing at all. |

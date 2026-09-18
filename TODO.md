# 📝 shotGun - TODO & Roadmap

## 🎯 v0.2.0 (Completed)
- [x] **Page 0 / 0-Indexed Support**: Allow starting counter at `0` (e.g. `shot_000.png` for cover pages/prefaces) or `1`.
- [x] **Interactive Session Reset Dialog**: Prompt modal to confirm/edit destination path, session name, file prefix, and starting number on session reset.
- [x] **System Tray Integration**: Minimize to tray with custom icon and context menu (*Show/Hide*, *Capture*, *New Session*, *Exit*).
- [x] **Windows Auto-Start Toggle**: Registry integration with `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- [x] **Compact UI Redesign**: Slim, streamlined window layout (~580x440 px).
- [x] **Multi-Monitor & ROI Crop**: Dynamic monitor detection, live freeze-frame snipping tool, and manual coordinate controls.
- [x] **Global Hotkey Engine**: Zero-polling Win32 `RegisterHotKey` loop for capture and session reset.
- [x] **Multi-Format Export**: PNG, JPEG with quality slider, BMP, and WebP encoders.
- [x] **Capture History & Clipboard**: Real-time history with instant open, locate, and copy to clipboard.
- [x] **Rebranded for Noerotech**.

---

## 🚀 v0.3.0 (Current Release - Completed)
- [x] **Video Recording (MP4)**: Continuous ROI capture via direct DXGI Desktop Duplication (`src/dxgi_capture.rs`), correctly multi-adapter-aware (works on hybrid-GPU laptops where a monitor isn't on the "default" adapter). Async PNG-encoder worker pool decouples capture from disk/CPU encode time. Real per-frame durations (not an averaged framerate) muxed via an ffmpeg concat script, resampled to a constant output fps.
- [x] **Desktop Audio Capture (WASAPI Loopback)**: `src/audio.rs` captures system audio directly via WASAPI (bypassing `cpal`, which doesn't support loopback mode); muxed in only when real samples were captured, degrading gracefully to video-only otherwise.
- [x] **Video hotkeys & UI**: Start/Stop Record button + hotkeys (default `F12`/`Shift+F12`), Target FPS and ffmpeg path settings, hidden ffmpeg console window, optional cleanup of intermediate PNG/WAV files after a successful encode.
- [x] **Show/Hide Window hotkey**: guaranteed way to restore the window (default `Ctrl+Alt+Insert`) independent of the system tray icon.
- [x] **Multi-Screenshot PDF Export**: manual "Export to PDF" button on the History tab, plus an opt-in auto-export when a session ends (`src/pdf_export.rs`, via `printpdf`). Individual images are always kept — the PDF is additional.

---

## ✅ Recently Fixed
- [x] **Post-capture annotation editor**: the Quick Region Capture flow (`Ctrl+Alt+Home`) now flows straight from drag-select into an annotate step on the same freeze overlay before saving — Arrow, Rectangle, Highlighter, Text, and Blur/Obfuscate (pixelate), a color picker, a full undo stack (toolbar button and Ctrl+Z), and Save/Copy/Cancel (also `Ctrl+S` / `Ctrl+Shift+S`). Scoped to Quick Capture only, deliberately — the plain "Drag-Select ROI" button and the plain Capture hotkey both define/reuse a *saved* region for repeated fast captures with no overlay involved at all, and forcing an annotate step onto either would break that. Implementation notes:
  - `src/annotate.rs`: the annotation list (`Vec<Annotation>`, in region-local physical-pixel space) and a hand-rolled rasterizer (`bake()`) — thick-line+triangle arrows, bordered rectangles, alpha-blended highlights, block-averaged pixelation for Blur, and a hand-authored 5x7 bitmap font for Text (one case only, lowercase renders as uppercase) so no new font-rendering crate or bundled/licensed font file was needed.
  - The overlay retains the frozen frame's real pixels (not just the GPU texture) specifically so `Save`/`Copy` have something to crop and bake into.
  - `capture::execute_capture` was split so the "write to disk with the session's naming/counter logic" half (`save_captured_image`) is reusable directly with an already-in-hand (annotated) image, without re-capturing the screen.
  - Confirmed with real input end-to-end (hotkey → drag → draw an arrow → Ctrl+S), then reading the saved PNG's pixels back to verify the arrow's exact color landed at the exact expected coordinates, not just that a file appeared.
- [x] **Progress spinner + async ffmpeg mux/PDF export**: `stop_video()` used to block the whole UI thread until ffmpeg finished muxing (`VideoHandle::stop()` joined the worker thread). It now just signals the stop and returns immediately; the worker thread sends its result asynchronously, polled every frame without blocking. PDF export (manual and auto-on-session) now runs on a background thread the same way. Both show a spinner (status bar, and next to the Record button while encoding) instead of freezing with no feedback.
- [x] **Wired up "Keep running in System Tray when minimized"**: minimizing now also hides the taskbar button (via the `WS_EX_TOOLWINDOW` extended style, toggled through the raw Win32 window handle) when the setting is on, so the window is only reachable via the tray icon while minimized — and the taskbar button always comes back on restore, regardless of the setting, so it can never get stranded. Deliberately doesn't touch `ViewportCommand::Visible` (see BUGS.md #8) — `WS_EX_TOOLWINDOW` only affects taskbar/alt-tab presentation, not whether the window keeps receiving repaints.
- [x] **Magnifier Loupe**: an 8x zoomed loupe with a center crosshair now follows the cursor while drag-selecting a region, sampled directly from the already-frozen screenshot texture — for lining up an edge to the exact pixel.
- [x] **Recording indicator**: a live "🔴 REC 00:01:23" timer (pulsing dot) now shows next to the Stop Video button while recording. Scoped down from the originally-planned full-screen pulsating border overlay on the recorded monitor — that version needs a second always-on-top, click-through, transparent window positioned per-monitor *and* excluded from DXGI capture (`SetWindowDisplayAffinity`) so it doesn't show up in the recording itself; real but riskier follow-up work, tracked separately below.
- [x] **Quick Region Capture hotkey**: new configurable hotkey (default `Ctrl+Alt+Home`, Hotkeys tab) that opens the drag-select overlay on whichever monitor the mouse cursor is currently on, then captures immediately once the region is confirmed — no separate Capture press needed, and no need to pre-select a Target Monitor.
- [x] **History: remove a single item**: added a 🗑️ Remove button per entry in the History tab, alongside the existing "Clear" (all). Removes it from the list only — the saved file on disk is untouched, matching Clear's existing behavior.
- [x] **Drag-select ROI on a mixed-DPI monitor (e.g. a 150% middle display between two 100% ones)**: the overlay used to end up sized/positioned wrong and overlapping the neighboring monitor, and the app window's own size came back wrong after cancelling. Fixed by handing the actual sizing to real OS borderless fullscreen instead of computing it by hand across the DPI boundary. See BUGS.md #10.
- [x] **Drag-select ROI now covers the real physical monitor, not just the small app window**: "🎯 Drag-Select ROI on Screen" used to draw the freeze-frame overlay inside whatever size the app window happened to be, making precise selection awkward on a small window. It now temporarily borderless-fullscreens the app window over the actual target monitor (1:1 physical pixel scale) before starting the drag-select, and restores the window's original position/size/decorations afterward. See BUGS.md #9.
- [x] **"Copy to clipboard" opt-in on capture**: added a Settings checkbox ("Automatically copy each screenshot to clipboard on capture", off by default) so every capture can optionally also go straight to the clipboard, in addition to the existing per-item 📋 Copy button in History.
- [x] **Window disappearing + app going fully unresponsive to tray/hotkeys**: `toggle_window_visibility` used `ViewportCommand::Visible(false)` to hide the window, which stops Windows from ever delivering `RedrawRequested` again — starving egui's `update()` loop, which is what drains the hotkey/tray/capture channels. The first hide permanently bricked all of those. Fixed by hiding via `Minimized(true)`/`Minimized(false)` only, never `Visible(false)`. See BUGS.md #8.

---

## 🔧 Outstanding / Near-term
- [ ] **Full-screen pulsating ROI border overlay during recording**: the deferred half of the original "Recording HUD" item (the live timer pill is done — see Recently Fixed). Needs a second `egui` viewport over the recorded monitor (`.with_transparent`, `.with_always_on_top`, `.with_mouse_passthrough` — all supported cross-platform by egui 0.29's `ViewportBuilder`), positioned/sized to the ROI using the same mixed-DPI-safe approach as the drag-select overlay, *and* excluded from DXGI capture via `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` so the border itself doesn't end up in the recording.
- [ ] **In-process H.264 encoding**: replace the external `ffmpeg` dependency with an in-process encoder (e.g. `openh264`) + muxer, so video recording works without requiring ffmpeg to be installed/bundled.
- [ ] **WASAPI silent-gap handling**: loopback capture only delivers packets while something is actively rendering audio; a completely silent stretch mid-recording may not get continuous padding, which can drift audio/video sync on quiet recordings — needs a dummy silent render stream to keep the audio engine "awake," or explicit gap-filling.
- [ ] **Copy recorded video clip to clipboard**: mirror the existing screenshot "📋 Copy to Clipboard" for finished video recordings. Video has no standard "paste video data" clipboard format the way images do, so the practical version is copying the **file itself** (like Ctrl+C on a file in Explorer, so it pastes as a file into Discord/Slack/email) via the Windows `CF_HDROP` clipboard format — `arboard` doesn't support this, needs raw Win32 clipboard calls. Also needs a video-history list in the UI first (finished recordings currently only show up as a status-bar message, not a list like screenshots get in the History tab) to have somewhere to put the button.

---

## 🔮 v0.4.0 (Future Vision)
- [ ] **LLM Vision Session Analysis**: Send captured session screenshots to Gemini / Claude / OpenAI / local Ollama endpoint with user instructions to generate Markdown documentation, summaries, or structured data.
- [ ] **Cross-Platform Support**: Linux (X11 / Wayland) and macOS (CoreGraphics & Accessibility API).

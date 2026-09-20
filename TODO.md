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
- [x] **Annotate toolbar's "Copy" now also saves to disk and shows in History**: previously clipboard-only with no disk write at all — the *only* place in the app where copying didn't also mean saving, which was surprising rather than useful. Now "Copy" is "Save" plus an explicit clipboard copy (same underlying `save_captured_image` + History insert as everywhere else), regardless of the `auto_copy_to_clipboard` setting.
- [x] **Annotation editor: fixed 3 real bugs found by actual use, added Step Label + Redact tools + blur strength**:
  - **Fixed**: the live preview drew a stale diagonal line (in the default red Arrow color) between the *selection* drag's leftover start/end points, the instant annotate mode opened — `start_pos`/`current_pos` weren't cleared on that transition.
  - **Fixed**: the arrow preview drew a plain dot (`circle_filled`) at the tip instead of a triangular arrowhead. (The actual saved file already had a correct triangle via `annotate::draw_arrow` — this was preview-only.)
  - **Fixed**: the toolbar and the drawing canvas are separate same-priority (`Order::Foreground`) UI layers; dragging on the canvas could bump it above the toolbar, burying its buttons so clicks stopped registering. The toolbar now force-raises itself to the top every frame via `ctx.move_to_top`.
  - **Added Step Label tool** (`①` button / `6`): click to place a numbered badge (1, 2, 3, ...) — for tutorial-style callouts. Auto-increments per placement; undo hands the number back so undo-then-replace doesn't skip ahead. Uses the same color picker as the other tools, so different badges can be different colors.
  - **Added Redact tool** (`⬛` button / `7`): a fully opaque solid-fill box, for when blur isn't foolproof enough and the content needs to be completely gone rather than just illegible.
  - **Blur strength slider**: the pixelation block size (previously a fixed 12px, not aggressive enough for some text) is now a 0-100% slider in the toolbar, mapping to a 6-40px block, defaulting to 65%.
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
Roughly in the order it makes sense to tackle them — small/contained fixes first, then the two sizable pre-profile items, with Capture Profiles (below) as the next big feature after this list.

1. - [ ] **Version string is stale**: window title, tray tooltip, and the About tab all still say "v0.3.0" even though several feature releases' worth of work has shipped since (Quick Capture, the annotation editor, async encoding, tray-minimize, etc.). Cheap fix, worth doing before anything else so version numbers mean something again — bump to v0.4.0 once Capture Profiles below ship, or cut a v0.3.1 now for everything already in `Recently Fixed`.
2. - [ ] **Wire up or remove `close_to_tray`**: `AppConfig::close_to_tray` exists and is serialized, but nothing reads it and there's no checkbox for it in Settings — it's silently dead. Either give it a real "close (X) button minimizes to tray instead of exiting" behavior (mirroring the `minimize_to_tray` + `WS_EX_TOOLWINDOW` approach, intercepting `ViewportEvent::Close` and cancelling it via `ViewportCommand::CancelClose`) or delete the field.
3. - [ ] **Encoding-in-progress app exit is unsafe**: since `stop_video()` became non-blocking (async ffmpeg mux), quitting the app while `video_encoding` is true kills the encode thread mid-write, which can leave a corrupt/truncated MP4. `TrayAction::ExitApp` and the window close path should check `self.video_encoding` (and any in-flight PDF export) and either block with a "still encoding, please wait" status or defer the actual `ViewportCommand::Close` until the async result lands.
4. - [ ] **`video::tests` flake when run together**: 3 of the `video::tests::*` tests fail when the full suite runs concurrently but pass individually — looks like resource contention between simultaneous DXGI duplication sessions in the test process, not a real product bug. Worth a `#[serial]` (via the `serial_test` crate) or a shared test mutex so CI/`cargo test` is reliable.
5. - [ ] **Annotation Text tool is uppercase-only, blocky font**: acceptable for v1 (avoids bundling a licensed font / new dependency — see BUGS.md), but worth a real lower-case glyph table at some point since it currently uppercases everything typed.
6. - [ ] **Annotation editor is Quick-Capture-only**: deliberate scope for v1 (see `Recently Fixed`), but worth an opt-in Settings toggle ("Always annotate after Drag-Select ROI") for people who want it on the manual selection flow too, without changing the plain Capture hotkey's fast/no-overlay path.
7. - [ ] **Full-screen pulsating ROI border overlay during recording**: the deferred half of the original "Recording HUD" item (the live timer pill is done — see Recently Fixed). Needs a second `egui` viewport over the recorded monitor (`.with_transparent`, `.with_always_on_top`, `.with_mouse_passthrough` — all supported cross-platform by egui 0.29's `ViewportBuilder`), positioned/sized to the ROI using the same mixed-DPI-safe approach as the drag-select overlay, *and* excluded from DXGI capture via `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` so the border itself doesn't end up in the recording.
8. - [ ] **Copy recorded video clip to clipboard**: mirror the existing screenshot "📋 Copy to Clipboard" for finished video recordings. Video has no standard "paste video data" clipboard format the way images do, so the practical version is copying the **file itself** (like Ctrl+C on a file in Explorer, so it pastes as a file into Discord/Slack/email) via the Windows `CF_HDROP` clipboard format — `arboard` doesn't support this, needs raw Win32 clipboard calls. Also needs a video-history list in the UI first (finished recordings currently only show up as a status-bar message, not a list like screenshots get in the History tab) to have somewhere to put the button.
9. - [ ] **WASAPI silent-gap handling**: loopback capture only delivers packets while something is actively rendering audio; a completely silent stretch mid-recording may not get continuous padding, which can drift audio/video sync on quiet recordings — needs a dummy silent render stream to keep the audio engine "awake," or explicit gap-filling.
10. - [ ] **In-process H.264 encoding**: replace the external `ffmpeg` dependency with an in-process encoder (e.g. `openh264`) + muxer, so video recording works without requiring ffmpeg to be installed/bundled.

---

## 🔮 v0.4.0 (Next Major Feature: Capture Profiles)

**Goal**: switch between named, self-contained capture setups instantly — e.g. a "Teams" profile (a fixed region of the screen, saved straight to a OneDrive-synced folder with a `teams_` prefix) and a "Video Recording" profile (a different region/monitor, its own output folder and fps), without manually re-pointing Target Monitor / ROI / output directory / filename every time you switch what you're doing.

### What a profile owns vs. what stays global
Splitting `AppConfig` this way keeps the change contained — `do_capture()`/`start_video()`/`save_captured_image()` etc. keep reading the same flat fields they always have; only *which* profile's values are currently loaded into those fields changes.

- **Per-profile** (this is the new `CaptureProfile` struct): `name`, `kind` (Screenshot or Video — see below), `monitor_index` + `monitor_name` (matched by name first with index as a fallback/cache, so a monitor reconnected in a different port order doesn't silently point a profile at the wrong screen), `region`, `output_dir`, `file_prefix`, `padding_digits`, `counter` + `start_index` + `session_index` + `session_prefix` + `use_session_subfolders` (so switching profiles and back resumes *that* profile's own numbering, not whatever another profile left the shared counter at), `format` + `jpeg_quality` (Screenshot profiles), `video_fps` + `cleanup_video_frames_after_encode` (Video profiles), `auto_copy_to_clipboard`, `auto_export_pdf_on_session` (both make sense to vary per profile — e.g. auto-copy for a "Teams" profile you're about to paste into chat, off for a "Bug Reports" profile going straight to a tracker).
- **Stays global** (one setting for the whole app, not duplicated per profile): all hotkey bindings, `minimize_to_tray`, `close_to_tray`, `prompt_on_new_session`, `play_sound`, `overwrite_existing`, `ffmpeg_path`, autostart. These are about how the *app* behaves, not what a given capture setup looks like.

### Storage & migration
- New `profiles.json` next to the existing `config.json` (same `directories::ProjectDirs` config dir): `{ "active_profile_id": "...", "profiles": [ ... ] }`.
- On first run after upgrading (no `profiles.json` yet), auto-create a single `"Default"` profile from whatever the current `config.json` already has, so existing users don't lose their setup or get silently reset — this also means Phase 1 can ship with zero visible UI change for someone who never opens the Profiles tab.

### UI
- A row of profile "pills" across the top of the app (Screen tab, above the existing Target Monitor / ROI card): `[Teams] [Video Recording] [+ New]`. Clicking one switches — write the *current* live settings back into whichever profile was active before switching (so nothing typed is lost), then load the newly-selected profile's saved values into the live config.
- A small always-visible badge (status bar, next to the ROI/monitor summary) naming the active profile, so it's never ambiguous which context is live.
- "Manage Profiles": rename, duplicate (handy for "Teams" → "Teams (large region)"), delete (with a confirm — this is a destructive action per the house rules on destructive UI actions), reorder. Duplicating or creating new can start from "blank" or "copy of current live settings."
- The profile editor itself just reuses the existing Screen/Output tab's widgets (monitor picker, drag-select ROI, output dir browse, prefix/padding, format/quality *or* fps/cleanup depending on `kind`) — no new input widgets need inventing, just a home for them per-profile instead of one shared set.

### Phasing
1. **Data + switch mechanics** (no UI): `CaptureProfile` struct, `profiles.json` load/save, `apply_profile_to_config()` / `capture_profile_from_config()`, the Default-profile migration. Testable headlessly.
2. **UI**: profile pills + switch behavior, Manage Profiles (create/duplicate/delete/rename), active-profile badge.
3. **Polish**: confirm per-profile counters survive app restarts correctly; per-profile `auto_copy_to_clipboard`/`auto_export_pdf_on_session` wired through; tray menu gains a "Switch Profile" submenu listing all profiles for a one-click switch without opening the window.
4. **Stretch, likely a later release**: a dedicated hotkey per profile (e.g. hold-and-select or `Ctrl+Alt+1..9`) to switch *and* immediately capture in one press, without touching the UI at all — this is the natural companion to Quick Region Capture once profiles exist, but adds real hotkey-registration complexity (up to 9+ more global hotkeys to manage, conflict-check, and expose in the UI) so it's deliberately not in the MVP.

### Open question to settle before implementation
Should **Quick Region Capture** (cursor-monitor-aware snip) write its picked region back into the *active* profile automatically, or only ever update the live/current settings without silently mutating a saved profile? Leaning towards the latter (explicit "Update active profile from current settings" action instead) so switching to "Teams" and then quick-capturing somewhere else doesn't quietly change what "Teams" means — but worth confirming against how you'd actually expect to use it before building it either way.

---

## 🔮 v0.5.0+ (Further Out)
- [ ] **LLM Vision Session Analysis**: Send captured session screenshots to Gemini / Claude / OpenAI / local Ollama endpoint with user instructions to generate Markdown documentation, summaries, or structured data.
- [ ] **Cross-Platform Support**: Linux (X11 / Wayland) and macOS (CoreGraphics & Accessibility API).

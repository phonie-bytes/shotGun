# 📝 shotGun - TODO & Roadmap

## 🎯 v0.1.0 (Current Release - Completed)
- [x] Multi-monitor detection and selection dropdown with resolution, scale factor, and position coordinates.
- [x] Interactive snipping overlay: freeze-frame background with drag-to-select rectangle bounding box and dimension display.
- [x] Manual ROI numeric coordinates input with quick presets (Top/Bottom half, Left/Right half, Center 50%).
- [x] Native Win32 global hotkeys using `RegisterHotKey` in a dedicated low-overhead thread.
- [x] Configurable hotkey combinations (Modifiers: Ctrl, Alt, Shift, Win + Keys: F1-F12, Letters, Numbers, PrintScreen, etc.).
- [x] Capture Hotkey and "Start New Session / Reset Counter" Hotkey.
- [x] Multi-format image export: Lossless PNG, JPEG (with quality slider 1-100), BMP, and lossless WebP.
- [x] Sequential filename incrementing with customizable prefix and zero-padding (e.g. `shot_001.png`, `shot_002.png`).
- [x] Session subfolder generation option (`captures/session_01/shot_001.png`).
- [x] Audio chime feedback upon capture (Windows standard MessageBeep).
- [x] Capture History list with timestamps, file sizes, resolutions, "Open Image", "Locate in Explorer", and "Copy to Clipboard".
- [x] Persistent JSON configuration saving and loading (`config.json`).
- [x] Unit test suite for hotkey calculations, serialization, and region cropping math.

---

## 🚀 v0.2.0 (Near-term Roadmap)
- [ ] **System Tray Integration**: Minimize shotGun to the Windows system tray with a right-click context menu to pause/resume hotkeys and quickly change modes.
- [ ] **Magnifier Loupe**: Add an 8x pixel magnifier loupe near the cursor during interactive drag-selection for pixel-perfect edge alignment.
- [ ] **Animated GIF / WebP Burst Recording**: Allow holding the capture hotkey to record a short animated GIF or WebP loop of the selected region.
- [ ] **On-screen Annotation Tools**: Add arrow, highlighter, text, rectangle, and blur/pixelate tools directly to the freeze-frame snipping tool before saving.
- [ ] **OCR (Optical Character Recognition)**: Add an optional "Copy Text from Region" mode using Tesseract or Windows Media OCR.

---

## 🔮 v0.3.0 (Future Vision)
- [ ] **Cross-Platform Support**: Linux (X11 / Wayland via `xcap` & `global-hotkey`) and macOS (CoreGraphics & Accessibility API).
- [ ] **Cloud & Webhook Uploads**: Automated upload to S3, Google Drive, Imgur, or custom HTTP webhooks with automatic URL copying to clipboard.
- [ ] **Delay Timer**: Configurable countdown timer (e.g. 3s, 5s) before capture for capturing transient context menus and tooltips.
- [ ] **Multi-Region Capture**: Define multiple ROI bounding boxes and capture all of them simultaneously into separate files with one hotkey press.

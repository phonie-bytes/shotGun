# 📝 shotGun - TODO & Roadmap

## 🎯 v0.2.0 (Current Release - Completed)
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

## 🚀 v0.3.0 (Near-term Roadmap)
- [ ] **Video Recording (MP4)**: Continuous frame grab from selected ROI (DXGI / Windows Graphics Capture) encoded to H.264 MP4.
- [ ] **Desktop Audio Capture (WASAPI Loopback)**: Capture system and application audio playing in the background, muxed into the video recording.
- [ ] **Recording HUD**: Pulsating red ROI border overlay + recording timer pill (`REC 00:01:24`).
- [ ] **Magnifier Loupe**: 8x pixel magnifier near cursor during interactive drag-selection for pixel-perfect edge alignment.

---

## 🔮 v0.4.0 (Future Vision)
- [ ] **On-Screen Annotation Tools**: Drawing toolbar on freeze overlay (Arrow, Rectangle, Highlighter, Text, and Blur sensitive data).
- [ ] **LLM Vision Session Analysis**: Send captured session screenshots to Gemini / Claude / OpenAI / local Ollama endpoint with user instructions to generate Markdown documentation, summaries, or structured data.
- [ ] **Cross-Platform Support**: Linux (X11 / Wayland) and macOS (CoreGraphics & Accessibility API).

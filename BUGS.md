# 🐛 shotGun - Known Bugs, Edge Cases & Troubleshooting

This document tracks known issues, platform-specific edge cases (notably Windows DPI scaling and multi-monitor layouts), and their resolutions or workarounds.

---

## 🔍 Known Edge Cases & Handling

### 1. Mixed High-DPI Multi-Monitor Setups
- **Symptom**: If Monitor 1 is 4K with 150% DPI scaling and Monitor 2 is 1080p with 100% DPI scaling, screenshot coordinates could misalign if physical vs logical pixel scales are confused.
- **How shotGun Handles This**:
  - Screen capture via `xcap` captures the raw physical pixel buffer directly from the display driver.
  - The region selector overlay calculates the scaling ratio between `screen_rect` in logical egui points and `monitor_width / monitor_height` in raw device pixels, ensuring exact bounding box alignment regardless of Windows display scaling settings.

### 2. Hotkey Conflicts with Other Applications
- **Symptom**: When registering a hotkey that is already claimed by another application (e.g. `PrintScreen` by Windows Snipping Tool or `F9` by an active game overlay), `RegisterHotKey` returns error code 0.
- **How shotGun Handles This**:
  - The hotkey background thread detects registration failures immediately and dispatches a `RegisteredStatus` error event.
  - The top bar UI turns red (`🔴 Failed to bind Capture - hotkey conflict`) and alerts the user to choose another modifier/key combination.

### 3. Negative Virtual Screen Coordinates in Multi-Monitor Layouts
- **Symptom**: Secondary monitors placed to the left or above the primary monitor have negative `(x, y)` virtual desktop coordinates in Windows (e.g. `x = -1920`).
- **How shotGun Handles This**:
  - `xcap` captures monitor textures in their local coordinate frame `(0..width, 0..height)`.
  - Region offsets are kept relative to the selected monitor's origin rather than the virtual desktop origin, preventing negative bounding box exceptions.

### 4. Overwrite Protection vs Continuous Captures
- **Symptom**: If the user resets the counter or modifies existing files in the output directory, existing screenshots could be overwritten accidentally.
- **How shotGun Handles This**:
  - shotGun includes an automatic file-collision scanner: if `overwrite_existing` is disabled, the capture engine scans ahead until an unused filename number is found.

---

## 🛠️ Troubleshooting Guide

| Issue | Cause | Solution |
|---|---|---|
| **Hotkeys not firing when in a game** | The game is running in exclusive fullscreen mode with elevated (Administrator) privileges. | Run shotGun as Administrator so Windows allows hotkey interception over elevated game windows. |
| **Black screen capture on protected DRM video** | Protected media playback (e.g. Netflix in Edge) uses hardware DRM overlay. | Disable hardware acceleration in the browser or capture unprotected content. |
| **File not saving** | Target directory is write-protected or read-only. | Use the **📁 Output & Naming** tab to select an accessible folder like `C:\Users\<user>\Pictures\shotGun`. |

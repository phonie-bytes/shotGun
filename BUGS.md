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

---

## 🛠️ Troubleshooting Guide

| Issue | Cause | Solution |
|---|---|---|
| **Hotkeys not firing when in a game** | The game is running in exclusive fullscreen mode with elevated (Administrator) privileges. | Run shotGun as Administrator so Windows allows hotkey interception over elevated game windows. |
| **Black screen capture on protected DRM video** | Protected media playback (e.g. Netflix in Edge) uses hardware DRM overlay. | Disable hardware acceleration in the browser or capture unprotected content. |
| **App hidden after closing** | "Keep running in System Tray when minimized" is enabled. | Double-click the shotGun tray icon near the Windows clock or right-click to select "Show / Hide shotGun". |

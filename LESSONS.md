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

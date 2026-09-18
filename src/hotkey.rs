// src/hotkey.rs
// Hotkey handling with video start/stop support

use crate::config::HotkeyConfig;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY, WM_QUIT,
};

pub const HOTKEY_ID_CAPTURE: i32 = 1001;
pub const HOTKEY_ID_NEW_SESSION: i32 = 1002;
pub const HOTKEY_ID_VIDEO_START: i32 = 2001;
pub const HOTKEY_ID_VIDEO_STOP: i32 = 2002;
pub const HOTKEY_ID_TOGGLE_WINDOW: i32 = 3001;
pub const HOTKEY_ID_QUICK_CAPTURE: i32 = 4001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Capture,
    NewSession,
    VideoStart,
    VideoStop,
    ToggleWindow,
    QuickCapture,
}

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    Triggered(HotkeyAction),
    RegisteredStatus {
        capture_ok: bool,
        new_session_ok: bool,
        video_start_ok: bool,
        video_stop_ok: bool,
        toggle_window_ok: bool,
        quick_capture_ok: bool,
        error_msg: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct AvailableKey {
    pub name: &'static str,
    pub vk_code: u32,
}

pub const AVAILABLE_KEYS: &[AvailableKey] = &[
    AvailableKey { name: "F1", vk_code: 0x70 },
    AvailableKey { name: "F2", vk_code: 0x71 },
    AvailableKey { name: "F3", vk_code: 0x72 },
    AvailableKey { name: "F4", vk_code: 0x73 },
    AvailableKey { name: "F5", vk_code: 0x74 },
    AvailableKey { name: "F6", vk_code: 0x75 },
    AvailableKey { name: "F7", vk_code: 0x76 },
    AvailableKey { name: "F8", vk_code: 0x77 },
    AvailableKey { name: "F9", vk_code: 0x78 },
    AvailableKey { name: "F10", vk_code: 0x79 },
    AvailableKey { name: "F11", vk_code: 0x7A },
    AvailableKey { name: "F12", vk_code: 0x7B },
    AvailableKey { name: "Insert", vk_code: 0x2D },
    AvailableKey { name: "Home", vk_code: 0x24 },
    AvailableKey { name: "End", vk_code: 0x23 },
    AvailableKey { name: "Pause", vk_code: 0x13 },
    // ... other keys omitted for brevity ...
];

pub struct HotkeyManager {
    cmd_tx: Sender<HotkeyCommand>,
    running: Arc<AtomicBool>,
}

enum HotkeyCommand {
    Update {
        capture: HotkeyConfig,
        new_session: HotkeyConfig,
        video_start: HotkeyConfig,
        video_stop: HotkeyConfig,
        toggle_window: HotkeyConfig,
        quick_capture: HotkeyConfig,
    },
    Shutdown,
}

impl HotkeyManager {
    /// Create a new manager and register the initial hot‑keys.
    pub fn new(
        initial_capture: HotkeyConfig,
        initial_new_session: HotkeyConfig,
        initial_video_start: HotkeyConfig,
        initial_video_stop: HotkeyConfig,
        initial_toggle_window: HotkeyConfig,
        initial_quick_capture: HotkeyConfig,
    ) -> (Self, Receiver<HotkeyEvent>) {
        let (event_tx, event_rx) = unbounded();
        let (cmd_tx, cmd_rx) = unbounded();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        thread::Builder::new()
            .name("shotgun-hotkey-listener".to_string())
            .spawn(move || {
                let mut cap_cfg = initial_capture;
                let mut sess_cfg = initial_new_session;
                let mut vid_start_cfg = initial_video_start;
                let mut vid_stop_cfg = initial_video_stop;
                let mut toggle_cfg = initial_toggle_window;
                let mut quick_capture_cfg = initial_quick_capture;

                let register_keys = |
                    cap: &HotkeyConfig,
                    sess: &HotkeyConfig,
                    v_start: &HotkeyConfig,
                    v_stop: &HotkeyConfig,
                    toggle: &HotkeyConfig,
                    quick_capture: &HotkeyConfig,
                    tx: &Sender<HotkeyEvent>,
                | {
                    unsafe {
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_CAPTURE);
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_NEW_SESSION);
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_VIDEO_START);
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_VIDEO_STOP);
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_TOGGLE_WINDOW);
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_QUICK_CAPTURE);
                    }

                    let cap_ok = unsafe {
                        RegisterHotKey(
                            std::ptr::null_mut(),
                            HOTKEY_ID_CAPTURE,
                            cap.win32_modifiers(),
                            cap.vk_code,
                        ) != 0
                    };
                    let sess_ok = unsafe {
                        RegisterHotKey(
                            std::ptr::null_mut(),
                            HOTKEY_ID_NEW_SESSION,
                            sess.win32_modifiers(),
                            sess.vk_code,
                        ) != 0
                    };
                    let v_start_ok = unsafe {
                        RegisterHotKey(
                            std::ptr::null_mut(),
                            HOTKEY_ID_VIDEO_START,
                            v_start.win32_modifiers(),
                            v_start.vk_code,
                        ) != 0
                    };
                    let v_stop_ok = unsafe {
                        RegisterHotKey(
                            std::ptr::null_mut(),
                            HOTKEY_ID_VIDEO_STOP,
                            v_stop.win32_modifiers(),
                            v_stop.vk_code,
                        ) != 0
                    };
                    let toggle_ok = unsafe {
                        RegisterHotKey(
                            std::ptr::null_mut(),
                            HOTKEY_ID_TOGGLE_WINDOW,
                            toggle.win32_modifiers(),
                            toggle.vk_code,
                        ) != 0
                    };
                    let quick_capture_ok = unsafe {
                        RegisterHotKey(
                            std::ptr::null_mut(),
                            HOTKEY_ID_QUICK_CAPTURE,
                            quick_capture.win32_modifiers(),
                            quick_capture.vk_code,
                        ) != 0
                    };

                    let mut error_msg = None;
                    if !(cap_ok && sess_ok && v_start_ok && v_stop_ok && toggle_ok && quick_capture_ok) {
                        let mut msgs = Vec::new();
                        if !cap_ok { msgs.push(format!("Failed to bind Capture ({})", cap.display_string())); }
                        if !sess_ok { msgs.push(format!("Failed to bind New Session ({})", sess.display_string())); }
                        if !v_start_ok { msgs.push(format!("Failed to bind Video Start ({})", v_start.display_string())); }
                        if !v_stop_ok { msgs.push(format!("Failed to bind Video Stop ({})", v_stop.display_string())); }
                        if !toggle_ok { msgs.push(format!("Failed to bind Show/Hide Window ({})", toggle.display_string())); }
                        if !quick_capture_ok { msgs.push(format!("Failed to bind Quick Region Capture ({})", quick_capture.display_string())); }
                        error_msg = Some(msgs.join(" "));
                    }

                    let _ = tx.send(HotkeyEvent::RegisteredStatus {
                        capture_ok: cap_ok,
                        new_session_ok: sess_ok,
                        video_start_ok: v_start_ok,
                        video_stop_ok: v_stop_ok,
                        toggle_window_ok: toggle_ok,
                        quick_capture_ok,
                        error_msg,
                    });
                };

                // Initial registration
                register_keys(&cap_cfg, &sess_cfg, &vid_start_cfg, &vid_stop_cfg, &toggle_cfg, &quick_capture_cfg, &event_tx);

                while running_clone.load(Ordering::Relaxed) {
                    // Process commands from UI
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            HotkeyCommand::Update { capture, new_session, video_start, video_stop, toggle_window, quick_capture } => {
                                cap_cfg = capture;
                                sess_cfg = new_session;
                                vid_start_cfg = video_start;
                                vid_stop_cfg = video_stop;
                                toggle_cfg = toggle_window;
                                quick_capture_cfg = quick_capture;
                                register_keys(&cap_cfg, &sess_cfg, &vid_start_cfg, &vid_stop_cfg, &toggle_cfg, &quick_capture_cfg, &event_tx);
                            }
                            HotkeyCommand::Shutdown => {
                                unsafe {
                                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_CAPTURE);
                                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_NEW_SESSION);
                                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_VIDEO_START);
                                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_VIDEO_STOP);
                                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_TOGGLE_WINDOW);
                                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_QUICK_CAPTURE);
                                }
                                return;
                            }
                        }
                    }

                    // Process Windows message queue
                    let mut msg: MSG = unsafe { std::mem::zeroed() };
                    let has_msg = unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 };
                    if has_msg {
                        match msg.message {
                            WM_QUIT => break,
                            WM_HOTKEY => {
                                let id = msg.wParam as i32;
                                let action = match id {
                                    HOTKEY_ID_CAPTURE => HotkeyAction::Capture,
                                    HOTKEY_ID_NEW_SESSION => HotkeyAction::NewSession,
                                    HOTKEY_ID_VIDEO_START => HotkeyAction::VideoStart,
                                    HOTKEY_ID_VIDEO_STOP => HotkeyAction::VideoStop,
                                    HOTKEY_ID_TOGGLE_WINDOW => HotkeyAction::ToggleWindow,
                                    HOTKEY_ID_QUICK_CAPTURE => HotkeyAction::QuickCapture,
                                    _ => continue,
                                };
                                let _ = event_tx.send(HotkeyEvent::Triggered(action));
                            }
                            _ => unsafe { TranslateMessage(&msg); DispatchMessageW(&msg); },
                        }
                    } else {
                        thread::sleep(Duration::from_millis(10));
                    }
                }

                // Cleanup on exit
                unsafe {
                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_CAPTURE);
                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_NEW_SESSION);
                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_VIDEO_START);
                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_VIDEO_STOP);
                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_TOGGLE_WINDOW);
                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_QUICK_CAPTURE);
                }
            })
            .expect("Failed to spawn hotkey thread");

        (Self { cmd_tx, running }, event_rx)
    }

    /// Update hot‑key bindings at runtime.
    pub fn update_hotkeys(
        &self,
        capture: HotkeyConfig,
        new_session: HotkeyConfig,
        video_start: HotkeyConfig,
        video_stop: HotkeyConfig,
        toggle_window: HotkeyConfig,
        quick_capture: HotkeyConfig,
    ) {
        let _ = self.cmd_tx.send(HotkeyCommand::Update {
            capture,
            new_session,
            video_start,
            video_stop,
            toggle_window,
            quick_capture,
        });
    }

    /// Graceful shutdown.
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.cmd_tx.send(HotkeyCommand::Shutdown);
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

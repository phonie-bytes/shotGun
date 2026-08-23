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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Capture,
    NewSession,
}

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    Triggered(HotkeyAction),
    RegisteredStatus {
        capture_ok: bool,
        new_session_ok: bool,
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
    AvailableKey { name: "PrintScreen", vk_code: 0x2C },
    AvailableKey { name: "ScrollLock", vk_code: 0x91 },
    AvailableKey { name: "Pause", vk_code: 0x13 },
    AvailableKey { name: "Space", vk_code: 0x20 },
    AvailableKey { name: "Enter", vk_code: 0x0D },
    AvailableKey { name: "Tab", vk_code: 0x09 },
    AvailableKey { name: "Insert", vk_code: 0x2D },
    AvailableKey { name: "Delete", vk_code: 0x2E },
    AvailableKey { name: "Home", vk_code: 0x24 },
    AvailableKey { name: "End", vk_code: 0x23 },
    AvailableKey { name: "PageUp", vk_code: 0x21 },
    AvailableKey { name: "PageDown", vk_code: 0x22 },
    AvailableKey { name: "A", vk_code: 0x41 },
    AvailableKey { name: "B", vk_code: 0x42 },
    AvailableKey { name: "C", vk_code: 0x43 },
    AvailableKey { name: "D", vk_code: 0x44 },
    AvailableKey { name: "E", vk_code: 0x45 },
    AvailableKey { name: "F", vk_code: 0x46 },
    AvailableKey { name: "G", vk_code: 0x47 },
    AvailableKey { name: "H", vk_code: 0x48 },
    AvailableKey { name: "I", vk_code: 0x49 },
    AvailableKey { name: "J", vk_code: 0x4A },
    AvailableKey { name: "K", vk_code: 0x4B },
    AvailableKey { name: "L", vk_code: 0x4C },
    AvailableKey { name: "M", vk_code: 0x4D },
    AvailableKey { name: "N", vk_code: 0x4E },
    AvailableKey { name: "O", vk_code: 0x4F },
    AvailableKey { name: "P", vk_code: 0x50 },
    AvailableKey { name: "Q", vk_code: 0x51 },
    AvailableKey { name: "R", vk_code: 0x52 },
    AvailableKey { name: "S", vk_code: 0x53 },
    AvailableKey { name: "T", vk_code: 0x54 },
    AvailableKey { name: "U", vk_code: 0x55 },
    AvailableKey { name: "V", vk_code: 0x56 },
    AvailableKey { name: "W", vk_code: 0x57 },
    AvailableKey { name: "X", vk_code: 0x58 },
    AvailableKey { name: "Y", vk_code: 0x59 },
    AvailableKey { name: "Z", vk_code: 0x5A },
    AvailableKey { name: "0", vk_code: 0x30 },
    AvailableKey { name: "1", vk_code: 0x31 },
    AvailableKey { name: "2", vk_code: 0x32 },
    AvailableKey { name: "3", vk_code: 0x33 },
    AvailableKey { name: "4", vk_code: 0x34 },
    AvailableKey { name: "5", vk_code: 0x35 },
    AvailableKey { name: "6", vk_code: 0x36 },
    AvailableKey { name: "7", vk_code: 0x37 },
    AvailableKey { name: "8", vk_code: 0x38 },
    AvailableKey { name: "9", vk_code: 0x39 },
    AvailableKey { name: "Num 0", vk_code: 0x60 },
    AvailableKey { name: "Num 1", vk_code: 0x61 },
    AvailableKey { name: "Num 2", vk_code: 0x62 },
    AvailableKey { name: "Num 3", vk_code: 0x63 },
    AvailableKey { name: "Num 4", vk_code: 0x64 },
    AvailableKey { name: "Num 5", vk_code: 0x65 },
    AvailableKey { name: "Num 6", vk_code: 0x66 },
    AvailableKey { name: "Num 7", vk_code: 0x67 },
    AvailableKey { name: "Num 8", vk_code: 0x68 },
    AvailableKey { name: "Num 9", vk_code: 0x69 },
];

pub struct HotkeyManager {
    cmd_tx: Sender<HotkeyCommand>,
    running: Arc<AtomicBool>,
}

enum HotkeyCommand {
    Update {
        capture: HotkeyConfig,
        new_session: HotkeyConfig,
    },
    Shutdown,
}

impl HotkeyManager {
    pub fn new(
        initial_capture: HotkeyConfig,
        initial_new_session: HotkeyConfig,
    ) -> (Self, Receiver<HotkeyEvent>) {
        let (event_tx, event_rx) = unbounded();
        let (cmd_tx, cmd_rx) = unbounded();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        thread::Builder::new()
            .name("shotgun-hotkey-listener".to_string())
            .spawn(move || {
                let mut current_capture = initial_capture;
                let mut current_new_session = initial_new_session;

                let register_keys = |cap: &HotkeyConfig,
                                     sess: &HotkeyConfig,
                                     event_tx: &Sender<HotkeyEvent>| {
                    unsafe {
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_CAPTURE);
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_NEW_SESSION);
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

                    let mut error_msg = None;
                    if !cap_ok || !sess_ok {
                        let mut msgs = Vec::new();
                        if !cap_ok {
                            msgs.push(format!("Failed to bind Capture ({}) - hotkey conflict.", cap.display_string()));
                        }
                        if !sess_ok {
                            msgs.push(format!("Failed to bind New Session ({}) - hotkey conflict.", sess.display_string()));
                        }
                        error_msg = Some(msgs.join(" "));
                    }

                    let _ = event_tx.send(HotkeyEvent::RegisteredStatus {
                        capture_ok: cap_ok,
                        new_session_ok: sess_ok,
                        error_msg,
                    });
                };

                // Initial registration
                register_keys(&current_capture, &current_new_session, &event_tx);

                while running_clone.load(Ordering::Relaxed) {
                    // Check commands from UI
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            HotkeyCommand::Update { capture, new_session } => {
                                current_capture = capture;
                                current_new_session = new_session;
                                register_keys(&current_capture, &current_new_session, &event_tx);
                            }
                            HotkeyCommand::Shutdown => {
                                unsafe {
                                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_CAPTURE);
                                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_NEW_SESSION);
                                }
                                return;
                            }
                        }
                    }

                    // Process Windows message queue
                    let mut msg: MSG = unsafe { std::mem::zeroed() };
                    let has_msg = unsafe {
                        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0
                    };

                    if has_msg {
                        if msg.message == WM_QUIT {
                            break;
                        } else if msg.message == WM_HOTKEY {
                            let hotkey_id = msg.wParam as i32;
                            if hotkey_id == HOTKEY_ID_CAPTURE {
                                let _ = event_tx.send(HotkeyEvent::Triggered(HotkeyAction::Capture));
                            } else if hotkey_id == HOTKEY_ID_NEW_SESSION {
                                let _ = event_tx.send(HotkeyEvent::Triggered(HotkeyAction::NewSession));
                            }
                        } else {
                            unsafe {
                                TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                        }
                    } else {
                        // Sleep 10ms to keep CPU usage < 0.1% while maintaining snappy hotkey response
                        thread::sleep(Duration::from_millis(10));
                    }
                }

                unsafe {
                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_CAPTURE);
                    UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID_NEW_SESSION);
                }
            })
            .expect("Failed to spawn hotkey thread");

        (Self { cmd_tx, running }, event_rx)
    }

    pub fn update_hotkeys(&self, capture: HotkeyConfig, new_session: HotkeyConfig) {
        let _ = self.cmd_tx.send(HotkeyCommand::Update {
            capture,
            new_session,
        });
    }

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

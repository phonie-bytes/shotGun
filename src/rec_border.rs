//! A pulsing red frame drawn around the area being recorded, so it's obvious
//! when a recording is live (and what it covers).
//!
//! It is four thin layered, click-through, topmost, no-activate windows — one
//! per edge — rather than one big transparent window, so it never sits over
//! the recorded content or eats mouse input. Every window is marked
//! `WDA_EXCLUDEFROMCAPTURE` *before* it is shown, which keeps it out of the
//! DXGI desktop-duplication frames, i.e. out of the recorded video. If
//! Windows refuses that (versions before 10 2004), no border is shown at all:
//! a border baked into the video is worse than no border.

use crate::config::RectRegion;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, RegisterClassW,
    SetLayeredWindowAttributes, SetWindowDisplayAffinity, ShowWindow, TranslateMessage, UnregisterClassW,
    LWA_ALPHA, MSG, PM_REMOVE, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// Border thickness in physical pixels.
const THICKNESS: i32 = 4;
/// Window class name, also what the tests search for (NUL-terminated UTF-16).
pub const CLASS_NAME: &str = "ShotgunRecordingBorder";
/// Pulse period; alpha eases between `ALPHA_MIN` and `ALPHA_MAX`.
const PULSE_SECS: f32 = 1.4;
const ALPHA_MIN: f32 = 90.0;
const ALPHA_MAX: f32 = 235.0;
/// `RGB(255, 40, 40)` as a GDI COLORREF (0x00BBGGRR).
const BORDER_COLORREF: u32 = 0x0028_28FF;

/// A rectangle in virtual-screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ScreenRect {
    /// The area a recording covers: the whole monitor, or the region inside it.
    pub fn for_recording(monitor_x: i32, monitor_y: i32, monitor_w: u32, monitor_h: u32, region: Option<RectRegion>) -> Self {
        match region {
            Some(r) => ScreenRect {
                x: monitor_x + r.x as i32,
                y: monitor_y + r.y as i32,
                w: r.width as i32,
                h: r.height as i32,
            },
            None => ScreenRect { x: monitor_x, y: monitor_y, w: monitor_w as i32, h: monitor_h as i32 },
        }
    }
}

/// The four edge strips (top, bottom, left, right) drawn just *inside* `rect`,
/// so a full-monitor recording still gets a visible border. The side strips
/// stop short of the corners so nothing is drawn twice (double-drawn corners
/// would pulse brighter than the rest). Regions too small for a border of
/// `t` shrink it rather than producing negative sizes.
pub fn border_rects(rect: ScreenRect, t: i32) -> [ScreenRect; 4] {
    let t = t.min(rect.w / 2).min(rect.h / 2).max(1);
    [
        ScreenRect { x: rect.x, y: rect.y, w: rect.w, h: t },
        ScreenRect { x: rect.x, y: rect.y + rect.h - t, w: rect.w, h: t },
        ScreenRect { x: rect.x, y: rect.y + t, w: t, h: (rect.h - 2 * t).max(0) },
        ScreenRect { x: rect.x + rect.w - t, y: rect.y + t, w: t, h: (rect.h - 2 * t).max(0) },
    ]
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// The visible border. Dropping it (or calling `stop`) tears the windows down.
pub struct RecordingBorder {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RecordingBorder {
    /// Shows the border around `rect` until dropped. Never fails loudly: if
    /// the windows can't be made capture-exclusive, it just shows nothing.
    pub fn start(rect: ScreenRect) -> Self {
        Self::start_inner(rect, true)
    }

    /// `exclude_from_capture: false` exists only so tests can prove the
    /// border *would* show up in a capture without the exclusion.
    fn start_inner(rect: ScreenRect, exclude_from_capture: bool) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("shotgun-rec-border".to_string())
            .spawn(move || unsafe { run(rect, exclude_from_capture, thread_stop) })
            .ok();
        RecordingBorder { stop, thread }
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for RecordingBorder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The window thread: creates the strips, pulses their opacity, pumps
/// messages, and destroys everything once `stop` is set.
unsafe fn run(rect: ScreenRect, exclude_from_capture: bool, stop: Arc<AtomicBool>) {
    let hinstance = GetModuleHandleW(std::ptr::null());
    let class_name = wide(CLASS_NAME);
    let brush = CreateSolidBrush(BORDER_COLORREF);

    let mut class: WNDCLASSW = std::mem::zeroed();
    class.lpfnWndProc = Some(wnd_proc);
    class.hInstance = hinstance;
    class.hbrBackground = brush;
    class.lpszClassName = class_name.as_ptr();
    // Registering twice (a second recording in the same run) just fails with
    // "class already exists", which is fine: we UnregisterClassW at the end
    // and the existing registration is identical anyway.
    RegisterClassW(&class);

    let mut windows: Vec<HWND> = Vec::with_capacity(4);
    for strip in border_rects(rect, THICKNESS) {
        if strip.w <= 0 || strip.h <= 0 {
            continue;
        }
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            strip.x,
            strip.y,
            strip.w,
            strip.h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            continue;
        }
        // Layered windows need an alpha set before they're visible at all.
        SetLayeredWindowAttributes(hwnd, 0, ALPHA_MAX as u8, LWA_ALPHA);
        // Exclude from capture *before* showing, so not even one frame can
        // contain the border. If this fails, bail out and show nothing.
        if exclude_from_capture && SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) == 0 {
            DestroyWindow(hwnd);
            for w in windows.drain(..) {
                DestroyWindow(w);
            }
            UnregisterClassW(class_name.as_ptr(), hinstance);
            DeleteObject(brush);
            return;
        }
        windows.push(hwnd);
    }
    for &hwnd in &windows {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }

    let started = Instant::now();
    let mut msg: MSG = std::mem::zeroed();
    while !stop.load(Ordering::SeqCst) {
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let phase = started.elapsed().as_secs_f32() / PULSE_SECS * std::f32::consts::TAU;
        let alpha = ALPHA_MIN + (ALPHA_MAX - ALPHA_MIN) * (0.5 + 0.5 * phase.sin());
        for &hwnd in &windows {
            SetLayeredWindowAttributes(hwnd, 0, alpha as u8, LWA_ALPHA);
        }
        std::thread::sleep(Duration::from_millis(33));
    }

    for hwnd in windows {
        DestroyWindow(hwnd);
    }
    UnregisterClassW(class_name.as_ptr(), hinstance);
    DeleteObject(brush);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowDisplayAffinity, GetWindowLongW, GetWindowRect, IsWindowVisible,
        GWL_EXSTYLE,
    };

    #[test]
    fn strips_tile_the_frame_without_overlap() {
        let r = ScreenRect { x: 100, y: 50, w: 400, h: 300 };
        let [top, bottom, left, right] = border_rects(r, 4);
        assert_eq!(top, ScreenRect { x: 100, y: 50, w: 400, h: 4 });
        assert_eq!(bottom, ScreenRect { x: 100, y: 346, w: 400, h: 4 });
        assert_eq!(left, ScreenRect { x: 100, y: 54, w: 4, h: 292 });
        assert_eq!(right, ScreenRect { x: 496, y: 54, w: 4, h: 292 });
        // Total painted area = perimeter ring, no double-counted corners.
        let area: i32 = [top, bottom, left, right].iter().map(|s| s.w * s.h).sum();
        assert_eq!(area, 400 * 300 - 392 * 292);
    }

    #[test]
    fn tiny_regions_shrink_the_border_instead_of_going_negative() {
        for r in [ScreenRect { x: 0, y: 0, w: 5, h: 5 }, ScreenRect { x: 0, y: 0, w: 1, h: 1 }, ScreenRect { x: 0, y: 0, w: 300, h: 3 }] {
            for s in border_rects(r, 4) {
                assert!(s.w >= 0 && s.h >= 0, "negative size for {r:?}: {s:?}");
            }
        }
    }

    #[test]
    fn recording_rect_is_monitor_relative() {
        let region = RectRegion { x: 10, y: 20, width: 300, height: 200 };
        assert_eq!(
            ScreenRect::for_recording(-1920, 0, 1920, 1080, Some(region)),
            ScreenRect { x: -1910, y: 20, w: 300, h: 200 }
        );
        assert_eq!(
            ScreenRect::for_recording(-1920, 0, 1920, 1080, None),
            ScreenRect { x: -1920, y: 0, w: 1920, h: 1080 }
        );
    }

    /// (rect, ex-style, affinity, visible) for every live border window.
    fn live_border_windows() -> Vec<(RECT, u32, u32, bool)> {
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> i32 {
            let out = &mut *(lparam as *mut Vec<(RECT, u32, u32, bool)>);
            let mut name = [0u16; 64];
            let n = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) as usize;
            if String::from_utf16_lossy(&name[..n]) == CLASS_NAME {
                let mut rect: RECT = std::mem::zeroed();
                GetWindowRect(hwnd, &mut rect);
                let mut affinity = 0u32;
                GetWindowDisplayAffinity(hwnd, &mut affinity);
                out.push((rect, GetWindowLongW(hwnd, GWL_EXSTYLE) as u32, affinity, IsWindowVisible(hwnd) != 0));
            }
            1
        }
        let mut found: Vec<(RECT, u32, u32, bool)> = Vec::new();
        unsafe {
            EnumWindows(Some(collect), &mut found as *mut _ as LPARAM);
        }
        found
    }

    #[test]
    #[serial]
    fn windows_are_click_through_topmost_capture_excluded_and_cleaned_up() {
        let region = ScreenRect { x: 200, y: 200, w: 500, h: 400 };
        let border = RecordingBorder::start(region);
        std::thread::sleep(Duration::from_millis(400));

        let live = live_border_windows();
        assert_eq!(live.len(), 4, "expected four edge windows");
        let expected = border_rects(region, THICKNESS);
        for (rect, ex, affinity, visible) in &live {
            assert!(*visible, "border window should be shown");
            assert_eq!(*affinity, WDA_EXCLUDEFROMCAPTURE, "must be excluded from capture");
            for flag in [WS_EX_TRANSPARENT, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW] {
                assert!(ex & flag != 0, "missing ex-style {flag:#x} (have {ex:#x})");
            }
            assert!(
                expected.iter().any(|e| e.x == rect.left && e.y == rect.top && e.w == rect.right - rect.left && e.h == rect.bottom - rect.top),
                "unexpected window rect ({}, {}, {}, {})", rect.left, rect.top, rect.right, rect.bottom
            );
        }

        drop(border);
        assert_eq!(live_border_windows().len(), 0, "windows must be destroyed on stop");
    }

    /// The real proof that the border stays out of recordings: grab a frame
    /// through the same DXGI desktop duplication the recorder uses and look at
    /// a border pixel. With the exclusion it must not be the border's red; as
    /// a control, *without* the exclusion it must be (otherwise this test
    /// couldn't tell the difference and would pass vacuously).
    #[test]
    #[serial]
    fn border_is_absent_from_dxgi_frames_but_visible_without_exclusion() {
        let monitors = crate::capture::get_monitors();
        let Some(mon) = monitors.first() else {
            println!("no monitors; skipping");
            return;
        };
        let rect = ScreenRect { x: mon.x + 300, y: mon.y + 300, w: 600, h: 400 };
        // Middle of the top strip, in monitor-local pixels.
        let (px, py) = (300 + 300, 300 + 1);

        let sample = |exclude: bool| -> Option<[u8; 3]> {
            let border = RecordingBorder::start_inner(rect, exclude);
            std::thread::sleep(Duration::from_millis(500));
            let session = crate::dxgi_capture::open_for_point(mon.x, mon.y).ok()?;
            let mut result = None;
            for _ in 0..20 {
                if let Ok(Some((w, _h, rgba))) = crate::dxgi_capture::acquire_frame(&session, 250) {
                    let i = ((py as u32 * w + px as u32) * 4) as usize;
                    result = Some([rgba[i], rgba[i + 1], rgba[i + 2]]);
                    break;
                }
            }
            drop(border);
            result
        };
        // Redness rather than an absolute colour: the strip is alpha-blended.
        let is_border_red = |p: [u8; 3]| p[0] as i32 - p[1].max(p[2]) as i32 > 60;

        let Some(without) = sample(false) else {
            println!("couldn't acquire a DXGI frame here; skipping");
            return;
        };
        assert!(is_border_red(without), "control failed: an un-excluded border should show in DXGI frames, got {without:?}");

        let with = sample(true).expect("second frame");
        assert!(!is_border_red(with), "the border leaked into a DXGI frame: {with:?}");
    }
}

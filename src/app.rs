use crate::autostart::{is_autostart_enabled, set_autostart};
use crate::capture::{
    execute_capture, get_monitors, save_captured_image, CaptureResult, MonitorInfo,
};
use crate::config::{AppConfig, OutputFormat, RectRegion};
use crate::hotkey::{
    AVAILABLE_KEYS, HotkeyAction, HotkeyEvent, HotkeyManager,
};
use crate::overlay::{OverlayAction, RegionSelectorOverlay};
use crate::pdf_export;
use crate::profiles::{CaptureProfile, ProfileKind, ProfilesFile};
use crate::tray::{TrayAction, TrayHandler};
use crate::video;
use crossbeam_channel::Receiver;
use egui::{
    Align, Button, CentralPanel, Color32, Context, Layout, RichText, ScrollArea, TopBottomPanel,
    Ui, ViewportCommand,
};

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
    SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_TOOLWINDOW,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    ScreenRegion,
    Hotkeys,
    OutputNaming,
    History,
    Settings,
    About,
}

pub struct ShotgunApp {
    pub config: AppConfig,
    pub monitors: Vec<MonitorInfo>,
    pub hotkey_manager: HotkeyManager,
    pub hotkey_rx: Receiver<HotkeyEvent>,
    pub hotkey_status: (bool, bool, bool, bool, bool, bool),
    pub hotkey_error: Option<String>,
    pub active_tab: ActiveTab,
    pub history: Vec<CaptureResult>,
    pub status_message: String,
    pub overlay: RegionSelectorOverlay,
    /// Window (outer position, inner size) saved while the app window is
    /// temporarily borderless-fullscreened over a monitor for drag-select,
    /// so it can be restored once the overlay closes.
    pub overlay_saved_window: Option<(egui::Pos2, egui::Vec2)>,
    /// Set for exactly one frame after `Decorations(false)` is sent for the
    /// drag-select overlay, so the move/resize/overlay-start can happen on
    /// the frame after decorations actually take effect.
    pub pending_fullscreen_overlay: Option<usize>,
    /// Set for exactly one frame after the approximate move onto the
    /// target monitor, so real OS fullscreen can be requested once that
    /// move has actually settled.
    pub pending_fullscreen_enter: Option<usize>,
    /// Set for exactly one frame while restoring the window after a
    /// drag-select overlay, so the inner-size restore can happen once the
    /// position move back to the origin monitor has settled.
    pub pending_restore_size: Option<egui::Vec2>,
    /// Set while a drag-select overlay was started by the Quick Region
    /// Capture hotkey (rather than the manual "Drag-Select ROI" button), so
    /// a capture fires automatically once the region is confirmed and the
    /// window has fully restored — no separate Capture press needed.
    pub pending_quick_capture: bool,
    pub tray_handler: Option<TrayHandler>,
    pub window_visible: bool,
    /// Tracks the OS-reported minimized state from the previous frame, so
    /// `maybe_toggle_taskbar_for_minimize` can act only on the transition
    /// (just-minimized / just-restored) rather than every frame.
    pub was_minimized: bool,
    pub autostart_state: bool,

    // Hotkey temp state
    pub temp_capture_key_idx: usize,
    pub temp_session_key_idx: usize,
    // Video hotkey temp state
    pub temp_video_start_key_idx: usize,
    pub temp_video_stop_key_idx: usize,
    // Show/Hide window hotkey temp state
    pub temp_toggle_window_key_idx: usize,
    // Quick Region Capture hotkey temp state
    pub temp_quick_capture_key_idx: usize,
    // Video capture handle
    pub video_handle: Option<video::VideoHandle>,
    pub video_result_rx: Option<Receiver<video::VideoResult>>,
    /// True while waiting on the async encode result after `stop_video()`,
    /// used to show a "stopping & encoding" spinner in the status bar.
    pub video_encoding: bool,
    /// When the current recording started, for the live "REC 00:01:23"
    /// timer shown next to the Stop Video button.
    pub recording_started_at: Option<std::time::Instant>,
    /// Async PDF export in flight, if any: (label to prefix the status
    /// message with once done, receiver for the result).
    pub pdf_export_rx: Option<(String, Receiver<Result<PathBuf, String>>)>,
    /// Set when the user asked to exit (native close button or tray Exit)
    /// while a video encode or PDF export was still in flight — closing
    /// then would kill the worker thread mid-write and can corrupt the
    /// output file. The actual `ViewportCommand::Close` is deferred until
    /// both `video_encoding` is false and `pdf_export_rx` is `None`.
    pub pending_exit: bool,
    /// An exit that's already been vetted (tray Exit, or a deferred exit
    /// whose encode/export has now finished) and must not be intercepted
    /// again by `handle_close_request` — otherwise, with `close_to_tray` on,
    /// the `Close` we send would itself be turned back into a minimize and
    /// an explicit "Exit" would never actually exit.
    pub exit_confirmed: bool,

    // Capture Profiles
    pub profiles: ProfilesFile,
    pub profile_manager_open: bool,
    /// Which profile the manager window's editor panel is currently showing.
    pub profile_editing_id: Option<String>,
    /// Set while the profile editor's own "🎯 Drag-Select Region" button
    /// has kicked off an overlay session — on `OverlayAction::Confirmed`,
    /// the region is written into *this* profile (by id) instead of (only)
    /// the live config. Distinct from the manual Screen-tab button and the
    /// Quick Capture flow, neither of which touch this.
    pub profile_region_picker_target: Option<String>,
    /// The live config's monitor_index from just before the profile
    /// editor's region picker temporarily pointed it at the
    /// profile-under-edit's monitor — restored once the picker resolves,
    /// so editing a non-active profile's region doesn't leave the actually
    /// active profile's monitor selection silently overwritten.
    pub profile_region_picker_prev_monitor: Option<usize>,
    /// Text buffer for the manager window's "+ New Profile" name field.
    pub profile_new_name: String,
    /// Profile awaiting delete confirmation in the manager window —
    /// deleting is destructive, so the trash button only arms this and a
    /// separate explicit "Yes, delete" click actually removes it.
    pub profile_pending_delete: Option<String>,

    // Interactive Session Reset Modal State
    pub session_modal_open: bool,
    pub temp_session_name: String,
    pub temp_session_path: PathBuf,
    pub temp_session_prefix: String,
    pub temp_session_start_counter: u64,
    pub temp_session_use_subfolder: bool,
}

impl ShotgunApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.window_rounding = 8.0.into();
        visuals.panel_fill = Color32::from_rgb(18, 22, 28);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(26, 31, 39);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(33, 40, 50);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 57, 72);
        visuals.widgets.active.bg_fill = Color32::from_rgb(64, 76, 96);
        cc.egui_ctx.set_visuals(visuals);

        let config = AppConfig::load();
        let monitors = get_monitors();

        // Loads profiles.json, or (first run after upgrading) creates it
        // with a single "Default" profile snapshotting whatever config.json
        // already had — zero disruption for anyone who never opens the
        // Profiles UI. Never applied back onto `config` here: config.json
        // is already the authoritative live state for whichever profile
        // was active when the app last closed.
        let active_monitor_name = monitors
            .get(config.monitor_index)
            .map(|m| m.name.clone())
            .unwrap_or_default();
        let profiles = ProfilesFile::load_or_migrate(&config, active_monitor_name);

        // Determine initial UI indices for hotkeys
        let temp_capture_key_idx = AVAILABLE_KEYS
            .iter()
            .position(|k| k.vk_code == config.capture_hotkey.vk_code)
            .unwrap_or(8); // Default F9
        let temp_session_key_idx = AVAILABLE_KEYS
            .iter()
            .position(|k| k.vk_code == config.new_session_hotkey.vk_code)
            .unwrap_or(9); // Default F10
        let temp_video_start_key_idx = AVAILABLE_KEYS
            .iter()
            .position(|k| k.vk_code == config.video_start_hotkey.vk_code)
            .unwrap_or(11); // Default F12
        let temp_video_stop_key_idx = AVAILABLE_KEYS
            .iter()
            .position(|k| k.vk_code == config.video_stop_hotkey.vk_code)
            .unwrap_or(11); // Default F12 (Shift+F12)
        let temp_toggle_window_key_idx = AVAILABLE_KEYS
            .iter()
            .position(|k| k.vk_code == config.toggle_window_hotkey.vk_code)
            .unwrap_or(12); // Default Insert
        let temp_quick_capture_key_idx = AVAILABLE_KEYS
            .iter()
            .position(|k| k.vk_code == config.quick_capture_hotkey.vk_code)
            .unwrap_or(13); // Default Home

        // Initialize hotkey manager with all six hotkeys
        let (hotkey_manager, hotkey_rx) = HotkeyManager::new(
            config.capture_hotkey.clone(),
            config.new_session_hotkey.clone(),
            config.video_start_hotkey.clone(),
            config.video_stop_hotkey.clone(),
            config.toggle_window_hotkey.clone(),
            config.quick_capture_hotkey.clone(),
        );

        let tray_handler = TrayHandler::new().ok();
        let autostart_state = is_autostart_enabled();

        // Session modal temporary state
        let temp_session_path = config.output_dir.clone();
        let temp_session_prefix = config.file_prefix.clone();
        let temp_session_name = format!("{}{:02}", config.session_prefix, config.session_index + 1);
        let temp_session_start_counter = config.start_index;
        let temp_session_use_subfolder = config.use_session_subfolders;

        Self {
            config,
            monitors,
            hotkey_manager,
            hotkey_rx,
            hotkey_status: (false, false, false, false, false, false),
            hotkey_error: None,
            active_tab: ActiveTab::ScreenRegion,
            history: Vec::new(),
            status_message: String::new(),
            overlay: RegionSelectorOverlay::new(),
            overlay_saved_window: None,
            pending_fullscreen_overlay: None,
            pending_fullscreen_enter: None,
            pending_restore_size: None,
            pending_quick_capture: false,
            tray_handler,
            window_visible: true,
            was_minimized: false,
            autostart_state,
            // hotkey UI indices
            temp_capture_key_idx,
            temp_session_key_idx,
            temp_video_start_key_idx,
            temp_video_stop_key_idx,
            temp_toggle_window_key_idx,
            temp_quick_capture_key_idx,
            video_handle: None,
            video_result_rx: None,
            video_encoding: false,
            recording_started_at: None,
            pdf_export_rx: None,
            pending_exit: false,
            exit_confirmed: false,
            profiles,
            profile_manager_open: false,
            profile_editing_id: None,
            profile_region_picker_target: None,
            profile_region_picker_prev_monitor: None,
            profile_new_name: String::new(),
            profile_pending_delete: None,
            // session modal state
            session_modal_open: false,
            temp_session_name,
            temp_session_path,
            temp_session_prefix,
            temp_session_start_counter,
            temp_session_use_subfolder,
        }
    }

    pub fn do_capture(&mut self) {
        match execute_capture(&mut self.config) {
            Ok(result) => {
                let mut msg = format!(
                    "📸 Captured #{:0width$} ({}x{}) -> {}",
                    result.counter,
                    result.width,
                    result.height,
                    result.file_path.file_name().unwrap_or_default().to_string_lossy(),
                    width = self.config.padding_digits
                );
                if self.config.auto_copy_to_clipboard {
                    match Self::copy_image_to_clipboard(&result.file_path) {
                        Ok(()) => msg.push_str(" (copied to clipboard)"),
                        Err(e) => msg.push_str(&format!(" (clipboard copy failed: {e})")),
                    }
                }
                self.status_message = msg;
                self.history.insert(0, result);
                if self.history.len() > 100 {
                    self.history.pop();
                }
            }
            Err(err) => {
                self.status_message = format!("❌ Capture error: {err}");
            }
        }
    }

    /// Finishes a Quick Region Capture that went through the annotate
    /// toolbar: `image` is already cropped to `region`'s size and already
    /// has the user's annotations baked in — no screen re-capture needed.
    /// Always saves to disk and lands in History, whether this came from
    /// "Save" or "Copy" — "Copy" being clipboard-only with no disk write
    /// and no History entry was the *only* place in the app where copying
    /// didn't also mean saving, which was more surprising than useful.
    /// `explicit_copy` (the toolbar's "📋 Copy" action) means "copy to
    /// clipboard regardless of the `auto_copy_to_clipboard` setting" — the
    /// rest of this mirrors `do_capture()`'s bookkeeping (naming/counter
    /// via `save_captured_image`, history, status message).
    fn finish_annotated_capture(&mut self, image: image::RgbaImage, explicit_copy: bool) {
        let monitor_name = self
            .monitors
            .get(self.config.monitor_index)
            .map(|m| m.name.clone())
            .unwrap_or_default();

        match save_captured_image(&mut self.config, &image, monitor_name) {
            Ok(result) => {
                let mut msg = format!(
                    "📸 Captured #{:0width$} ({}x{}) -> {} (annotated)",
                    result.counter,
                    result.width,
                    result.height,
                    result.file_path.file_name().unwrap_or_default().to_string_lossy(),
                    width = self.config.padding_digits
                );
                if explicit_copy || self.config.auto_copy_to_clipboard {
                    match Self::copy_image_data_to_clipboard(&image) {
                        Ok(()) => msg.push_str(" (copied to clipboard)"),
                        Err(e) => msg.push_str(&format!(" (clipboard copy failed: {e})")),
                    }
                }
                self.status_message = msg;
                self.history.insert(0, result);
                if self.history.len() > 100 {
                    self.history.pop();
                }
            }
            Err(err) => {
                self.status_message = format!("❌ Capture error: {err}");
            }
        }
    }

    pub fn start_video(&mut self) {
        if self.video_handle.is_some() {
            self.status_message = "⏺️ Video recording already in progress.".to_string();
            return;
        }
        if self.video_encoding {
            // Starting now would replace `video_result_rx` and silently
            // drop the previous recording's still-in-flight encode result.
            self.status_message = "⏳ Still encoding the previous recording — wait for that to finish first.".to_string();
            return;
        }
        crate::video::debug_log("app.rs: start_video() called");
        let (handle, result_rx) = video::start_from_global(&self.config);
        self.video_handle = Some(handle);
        self.video_result_rx = Some(result_rx);
        self.recording_started_at = Some(std::time::Instant::now());
        self.status_message = "⏺️ Video recording started.".to_string();
    }

    /// Signals the recording to stop and returns immediately — the actual
    /// encode outcome is picked up asynchronously by `poll_video_result`,
    /// called every frame from `update()`, so the UI never blocks (and
    /// keeps painting the "stopping & encoding" spinner) while ffmpeg runs.
    pub fn stop_video(&mut self) {
        match self.video_handle.take() {
            Some(handle) => {
                crate::video::debug_log("app.rs: stop_video() called, stopping handle");
                self.status_message = "⏳ Stopping & encoding video...".to_string();
                self.video_encoding = true;
                self.recording_started_at = None;
                handle.stop();
            }
            None => {
                self.status_message = "No video recording in progress.".to_string();
            }
        }
    }

    /// Polls the video encode result channel without blocking. Called every
    /// frame; a no-op unless `stop_video()` was called and the worker
    /// thread's ffmpeg step has since finished.
    fn poll_video_result(&mut self) {
        let Some(rx) = &self.video_result_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.status_message = match result {
                    video::VideoResult::Encoded { path, frame_count, has_audio } => format!(
                        "🎬 Video saved: {} ({frame_count} frames{})",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        if has_audio { ", with audio" } else { ", no audio captured" }
                    ),
                    video::VideoResult::EncodeFailed { frames_dir, frame_count, reason } => format!(
                        "⚠️ Recorded {frame_count} frames but couldn't encode MP4 ({reason}). Frames saved in {}",
                        frames_dir.display()
                    ),
                };
                self.video_encoding = false;
                self.video_result_rx = None;
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                crate::video::debug_log("app.rs: result channel disconnected without a result (video thread likely panicked)");
                self.status_message = "❌ Video capture failed unexpectedly (no result). Check shotgun_video_debug.log in %TEMP%.".to_string();
                self.video_encoding = false;
                self.video_result_rx = None;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
    }

    pub fn trigger_new_session(&mut self, ctx: &Context) {
        if self.config.prompt_on_new_session {
            // Restore window if minimized and open prompt modal
            self.window_visible = true;
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Focus);

            self.temp_session_path = self.config.output_dir.clone();
            self.temp_session_prefix = self.config.file_prefix.clone();
            self.temp_session_name = format!("{}{:02}", self.config.session_prefix, self.config.session_index + 1);
            self.temp_session_start_counter = self.config.start_index;
            self.temp_session_use_subfolder = self.config.use_session_subfolders;
            self.session_modal_open = true;
        } else {
            self.silent_new_session();
        }
    }

    pub fn silent_new_session(&mut self) {
        let ending_session = self.config.session_index;
        self.config.session_index += 1;
        self.config.counter = self.config.start_index;
        let _ = self.config.save();
        self.status_message = format!(
            "🔄 Started New Session #{} (Counter reset to {})",
            self.config.session_index, self.config.start_index
        );
        self.maybe_auto_export_session_pdf(ending_session);
    }

    pub fn apply_new_session_from_modal(&mut self) {
        let ending_session = self.config.session_index;
        self.config.output_dir = self.temp_session_path.clone();
        self.config.file_prefix = self.temp_session_prefix.clone();
        self.config.session_index += 1;
        self.config.counter = self.temp_session_start_counter;
        self.config.use_session_subfolders = self.temp_session_use_subfolder;
        let _ = self.config.save();

        self.session_modal_open = false;
        self.status_message = format!(
            "🔄 Configured Session #{}: Start = {}, Folder = {}",
            self.config.session_index,
            self.config.counter,
            self.config.output_dir.file_name().unwrap_or_default().to_string_lossy()
        );
        self.maybe_auto_export_session_pdf(ending_session);
    }

    /// If enabled in Settings, bundles the screenshots captured during
    /// `ending_session` into a PDF (in addition to keeping the individual
    /// image files) once that session is about to close out.
    fn maybe_auto_export_session_pdf(&mut self, ending_session: u64) {
        if !self.config.auto_export_pdf_on_session {
            return;
        }
        let items: Vec<CaptureResult> = self
            .history
            .iter()
            .filter(|item| item.session == ending_session)
            .rev()
            .cloned()
            .collect();
        if items.is_empty() {
            return;
        }
        let Some(dest_dir) = items[0].file_path.parent() else {
            return;
        };
        let dest_path = dest_dir.join(format!("{}{:02}_screenshots.pdf", self.config.session_prefix, ending_session));
        let label = format!("{} 📄 Also exported {} screenshots to ", self.status_message, items.len());
        self.spawn_pdf_export(items, dest_path, label);
    }

    /// Exports everything currently in the History tab to a single PDF
    /// (oldest capture first), letting the user pick the destination.
    /// Individual image files are never touched — this is purely additional.
    pub fn export_history_to_pdf(&mut self) {
        if self.history.is_empty() {
            self.status_message = "No screenshots in history to export.".to_string();
            return;
        }
        if self.pdf_export_rx.is_some() {
            self.status_message = "A PDF export is already in progress.".to_string();
            return;
        }
        let items: Vec<CaptureResult> = self.history.iter().rev().cloned().collect();

        let Some(dest_path) = rfd::FileDialog::new()
            .set_directory(&self.config.output_dir)
            .set_file_name("screenshots.pdf")
            .add_filter("PDF document", &["pdf"])
            .save_file()
        else {
            return; // user cancelled
        };

        let label = format!("📄 Exported {} screenshots to ", items.len());
        self.status_message = "⏳ Exporting to PDF...".to_string();
        self.spawn_pdf_export(items, dest_path, label);
    }

    /// Runs `pdf_export::export_images_to_pdf` on a background thread so
    /// the UI stays responsive for large histories, and stores the
    /// receiver for `poll_pdf_export_result` (called every frame) to pick
    /// up. `label` is prefixed onto the eventual "exported to <filename>"
    /// status message — building it now lets callers capture whatever
    /// status text already existed (e.g. the New Session message) before
    /// this overwrites it with the in-progress spinner text.
    fn spawn_pdf_export(&mut self, items: Vec<CaptureResult>, dest_path: PathBuf, label: String) {
        let (tx, rx) = crossbeam_channel::unbounded();
        thread::spawn(move || {
            let result = pdf_export::export_images_to_pdf(&items, &dest_path).map(|()| dest_path);
            let _ = tx.send(result);
        });
        self.pdf_export_rx = Some((label, rx));
    }

    /// Polls the PDF export result channel without blocking. Called every
    /// frame; a no-op unless `spawn_pdf_export` started one and it hasn't
    /// finished yet.
    fn poll_pdf_export_result(&mut self) {
        let Some((label, rx)) = &self.pdf_export_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(dest_path)) => {
                self.status_message = format!("{label}{}", dest_path.file_name().unwrap_or_default().to_string_lossy());
                self.pdf_export_rx = None;
            }
            Ok(Err(e)) => {
                self.status_message = format!("⚠️ PDF export failed: {e}");
                self.pdf_export_rx = None;
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.status_message = "⚠️ PDF export failed unexpectedly.".to_string();
                self.pdf_export_rx = None;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
    }

    /// Finds which configured monitor currently contains the mouse cursor,
    /// comparing against each `MonitorInfo`'s physical-pixel virtual-desktop
    /// bounds (`x`/`y`/`width`/`height`), which is the same coordinate space
    /// `GetCursorPos` reports in.
    fn monitor_index_at_cursor(&self) -> Option<usize> {
        let mut point = POINT { x: 0, y: 0 };
        let ok = unsafe { GetCursorPos(&mut point) };
        if ok == 0 {
            return None;
        }
        self.monitors.iter().position(|m| {
            point.x >= m.x
                && point.x < m.x + m.width as i32
                && point.y >= m.y
                && point.y < m.y + m.height as i32
        })
    }

    /// Hotkey entry point for "Quick Region Capture": snip on whichever
    /// monitor the mouse is currently on, then capture immediately once the
    /// region is confirmed, instead of leaving the picked region as a saved
    /// setting for some later, separate Capture press.
    fn trigger_quick_region_capture(&mut self, ctx: &Context) {
        let Some(monitor_index) = self.monitor_index_at_cursor() else {
            self.status_message = "Couldn't determine which monitor the cursor is on.".to_string();
            return;
        };

        if self.config.monitor_index != monitor_index {
            self.config.monitor_index = monitor_index;
            let _ = self.config.save();
        }

        self.start_drag_select_overlay(ctx);
        self.pending_quick_capture = true;
    }

    /// Kicks off expanding the app's own window into a borderless overlay
    /// covering the *actual* target monitor at (close to) 1:1 scale, then
    /// starting the freeze-frame drag-select. Without this, dragging happens
    /// inside whatever small size the app window currently is, which makes
    /// precise region selection awkward — this makes it behave like a real
    /// full-screen snipping tool instead.
    ///
    /// This only sends `Decorations(false)` here and defers the actual
    /// move/resize/overlay-start to the *next* frame (see
    /// `pending_fullscreen_overlay` handling in `update()`). Windows/winit
    /// can recreate the native window when decorations are toggled, and a
    /// position/size command sent in the same batch as that toggle is prone
    /// to being silently dropped mid-recreation.
    /// Entry point for the profile editor's "🎯 Drag-Select Region" button:
    /// same freeze-overlay mechanism as the plain Screen-tab button
    /// (`annotate_after_select=false` — no annotate step here either), but
    /// targets whichever monitor `profile_id`'s profile currently has
    /// selected, and on confirm writes the region into *that profile*
    /// rather than the live config. See the `OverlayAction::Confirmed`
    /// handling in `update()` for the other half of this.
    fn start_profile_region_picker(&mut self, ctx: &Context, profile_id: &str) {
        let Some(profile_monitor) = self.profiles.find(profile_id).map(|p| p.monitor_index) else {
            return;
        };
        self.profile_region_picker_target = Some(profile_id.to_string());
        self.profile_region_picker_prev_monitor = Some(self.config.monitor_index);
        self.config.monitor_index = profile_monitor;
        self.start_drag_select_overlay(ctx);
    }

    fn start_drag_select_overlay(&mut self, ctx: &Context) {
        if self.monitors.get(self.config.monitor_index).is_none() {
            self.status_message = "No monitor available to select on.".to_string();
            return;
        }

        if self.overlay_saved_window.is_none() {
            // Save outer position (so the window reappears where it was)
            // but *inner* size: restoring re-enables decorations, and
            // InnerSize is content size excluding chrome. Using outer size
            // there would add the title-bar/border thickness on top of
            // itself every time this overlay is used, growing the window.
            //
            // Stored as *physical pixels*, not points: points are only
            // meaningful relative to whatever scale factor is active when
            // they're later sent as a command, which may well be a
            // different monitor's scale by restore time on a mixed-DPI
            // setup. Physical pixels stay correct regardless of which
            // monitor's DPI context is currently active.
            let live_scale = ctx
                .input(|i| i.viewport().native_pixels_per_point)
                .unwrap_or(1.0)
                .max(0.01);
            let outer_pos = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min);
            let inner_size = ctx.input(|i| i.viewport().inner_rect).map(|r| r.size());
            if let (Some(pos), Some(size)) = (outer_pos, inner_size) {
                self.overlay_saved_window = Some((
                    egui::Pos2::new(pos.x * live_scale, pos.y * live_scale),
                    egui::Vec2::new(size.x * live_scale, size.y * live_scale),
                ));
            }
        }

        ctx.send_viewport_cmd(ViewportCommand::Decorations(false));
        self.pending_fullscreen_overlay = Some(self.config.monitor_index);
    }

    /// Second phase of `start_drag_select_overlay`, run on the frame after
    /// decorations were dropped: nudges the window's *position* roughly
    /// onto the target monitor. This deliberately does **not** try to also
    /// compute the right *size* here — chasing the correct points-to-pixels
    /// conversion by hand across a monitor boundary (dividing by whichever
    /// scale factor is "current," re-reading it after settling, etc.) kept
    /// racing winit/Windows' own DPI-context updates in subtle ways: a
    /// position move can itself flip the active scale factor before a size
    /// command sent in the same or a following frame gets applied, so the
    /// size ends up scaled by a factor that didn't apply when it was
    /// computed. Rather than fight that, the actual full-monitor sizing is
    /// handed to real OS fullscreen (`apply_pending_fullscreen_enter`),
    /// which snaps to whatever monitor the window ends up on using its own
    /// correct, native scale handling — no manual pixel/point math at all.
    /// The position here only needs to be *roughly* right (get the window's
    /// top-left onto the destination monitor), since fullscreen mode
    /// ignores the window's prior size entirely.
    fn apply_pending_fullscreen_overlay(&mut self, ctx: &Context) {
        let Some(monitor_index) = self.pending_fullscreen_overlay.take() else {
            return;
        };
        let Some(mon) = self.monitors.get(monitor_index).cloned() else {
            self.restore_window_after_overlay(ctx);
            return;
        };

        let current_scale = ctx
            .input(|i| i.viewport().native_pixels_per_point)
            .unwrap_or(mon.scale_factor)
            .max(0.01);
        let pos = egui::Pos2::new(mon.x as f32 / current_scale, mon.y as f32 / current_scale);

        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos));

        self.pending_fullscreen_enter = Some(monitor_index);
    }

    /// Third phase: now that the window has (approximately) landed on the
    /// target monitor, ask the OS to make it real borderless fullscreen —
    /// Windows resolves the exact size against whatever monitor the window
    /// is actually on and its real scale factor, natively, so there's
    /// nothing left for us to compute by hand. Then start the freeze-frame
    /// capture.
    fn apply_pending_fullscreen_enter(&mut self, ctx: &Context) {
        let Some(monitor_index) = self.pending_fullscreen_enter.take() else {
            return;
        };
        if self.monitors.get(monitor_index).is_none() {
            self.restore_window_after_overlay(ctx);
            return;
        }

        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);

        // Quick Region Capture is the only flow that should detour into
        // annotate mode after a confirmed drag — the plain "Drag-Select
        // ROI" button flow defines a *reusable* region for repeated fast
        // captures and must keep returning immediately.
        if let Err(e) = self.overlay.start(ctx, monitor_index, self.pending_quick_capture) {
            self.status_message = format!("Overlay error: {e}");
            self.restore_window_after_overlay(ctx);
        }
    }

    /// Restores the app window's normal position/decorations after a
    /// drag-select overlay (started by `start_drag_select_overlay`) closes.
    /// `overlay_saved_window` holds *physical pixels* (see
    /// `start_drag_select_overlay`), converted to points here using
    /// whatever scale factor is active right now — the overlay's target
    /// monitor, since the window hasn't moved back yet. The size restore is
    /// deferred one frame — see `apply_pending_restore_size` — for the same
    /// cross-monitor DPI settling reason the launch sequence defers its own
    /// resize: applying it correctly requires the window to have actually
    /// finished moving back to the origin monitor first; doing both in the
    /// same batch races that.
    fn restore_window_after_overlay(&mut self, ctx: &Context) {
        // Always safe to send, even if we never actually reached real
        // fullscreen (e.g. the overlay errored before `apply_pending_fullscreen_enter`
        // ran) — turning off fullscreen when it's already off is a no-op.
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));

        if let Some((pos_px, size_px)) = self.overlay_saved_window.take() {
            let current_scale = ctx
                .input(|i| i.viewport().native_pixels_per_point)
                .unwrap_or(1.0)
                .max(0.01);
            let pos = egui::Pos2::new(pos_px.x / current_scale, pos_px.y / current_scale);

            ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos));
            self.pending_restore_size = Some(size_px);
        }
    }

    /// Second half of `restore_window_after_overlay`: applies the saved
    /// (physical-pixel) inner size, converted to points using whatever
    /// scale factor is active now that the position move back has had a
    /// frame to settle onto the origin monitor. If this restore followed a
    /// Quick Region Capture (`pending_quick_capture`), the window has by
    /// now moved off the target monitor — safe to take the actual
    /// screenshot without our own overlay chrome being in it.
    fn apply_pending_restore_size(&mut self, ctx: &Context) {
        let Some(size_px) = self.pending_restore_size.take() else {
            return;
        };
        let current_scale = ctx
            .input(|i| i.viewport().native_pixels_per_point)
            .unwrap_or(1.0)
            .max(0.01);
        let size = egui::Vec2::new(size_px.x / current_scale, size_px.y / current_scale);

        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(ViewportCommand::Focus);

        if self.pending_quick_capture {
            self.pending_quick_capture = false;
            self.do_capture();
        }
    }

    pub fn toggle_window_visibility(&mut self, ctx: &Context) {
        self.window_visible = !self.window_visible;
        // Deliberately never send Visible(false): on Windows, a genuinely
        // hidden (WS_VISIBLE=false) window stops receiving RedrawRequested
        // entirely, which stops egui's update() loop from ever running
        // again — bricking the app so no hotkey, tray click, or menu action
        // can be processed, including the one meant to show it again.
        // Minimized windows don't have this problem (eframe still pumps
        // them), so "hide" is implemented as minimize instead.
        ctx.send_viewport_cmd(ViewportCommand::Minimized(!self.window_visible));
        if self.window_visible {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
    }

    /// Implements "Keep running in System Tray when minimized": when the
    /// window transitions to minimized (by any means — the native minimize
    /// button, Win+M, etc.) and the setting is on, also hides its taskbar
    /// button by adding the `WS_EX_TOOLWINDOW` extended style, so it
    /// disappears from the taskbar entirely and is only reachable via the
    /// tray icon. On the reverse transition (restored), the style is always
    /// removed again regardless of the current setting value, so a window
    /// that comes back is never left stranded without a taskbar entry.
    ///
    /// Deliberately does *not* use `ViewportCommand::Visible` for any of
    /// this — seeing BUGS.md #8, that stops the window from ever receiving
    /// `RedrawRequested` again, bricking the app. `WS_EX_TOOLWINDOW` only
    /// affects taskbar/alt-tab presentation, not `WS_VISIBLE`, so it doesn't
    /// have that problem: the window keeps painting and processing input
    /// exactly as a normal minimized window would.
    fn maybe_toggle_taskbar_for_minimize(&mut self, ctx: &Context, frame: &eframe::Frame) {
        let is_minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        if is_minimized == self.was_minimized {
            return;
        }
        self.was_minimized = is_minimized;

        if is_minimized {
            if self.config.minimize_to_tray {
                Self::set_taskbar_button_visible(frame, false);
            }
        } else {
            Self::set_taskbar_button_visible(frame, true);
        }
    }

    /// Handles the native window close request (the title bar ✕ button).
    /// Two independent reasons to intercept it rather than let it proceed:
    ///
    /// 1. A video encode or PDF export in flight — closing kills that
    ///    worker thread mid-write and can corrupt the output file,
    ///    regardless of `close_to_tray`. Deferred via `pending_exit`; see
    ///    the check in `update()` after the async polls.
    /// 2. `close_to_tray` is on — the ✕ button should minimize to tray
    ///    instead of exiting, the same as `minimize_to_tray` does for the
    ///    actual minimize button, using the same `WS_EX_TOOLWINDOW`
    ///    mechanism (never `ViewportCommand::Visible`, see BUGS.md #8).
    ///
    /// Either way, `ViewportCommand::CancelClose` stops the close from
    /// actually happening this frame; a later close attempt (another ✕
    /// click) generates a fresh close-requested event to react to again.
    fn handle_close_request(&mut self, ctx: &Context, frame: &eframe::Frame) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }

        // An exit that's already been vetted (tray Exit, or a deferred exit
        // whose encode/export has since finished) — let it through as-is.
        // Without this, with `close_to_tray` on, the `Close` we send would
        // be intercepted below and turned back into a minimize.
        if self.exit_confirmed {
            return;
        }

        // ✕ with close_to_tray on never exits the app, so it's safe even
        // mid-encode — just minimize. (Checked before the in-flight-work
        // guard below for exactly that reason: that guard exists to stop
        // an *exit* from killing a worker thread, which this isn't.)
        if self.config.close_to_tray {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.window_visible = false;
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            Self::set_taskbar_button_visible(frame, false);
            // Pre-empt maybe_toggle_taskbar_for_minimize's own transition
            // detection next frame — we've already hidden the taskbar
            // button ourselves, so its `is_minimized == was_minimized`
            // check should see them already matching and no-op.
            self.was_minimized = true;
            self.status_message = "Minimized to tray.".to_string();
            return;
        }

        if self.video_encoding || self.pdf_export_rx.is_some() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending_exit = true;
            self.status_message = "⏳ Waiting for video encode / PDF export to finish before exiting...".to_string();
        }
        // Otherwise: close_to_tray is off and nothing's in flight — don't
        // send CancelClose, let the close proceed normally.
    }

    /// Adds or removes `WS_EX_TOOLWINDOW` on the app's native window,
    /// which controls whether it has a taskbar button. Silently does
    /// nothing if the raw window handle isn't available (e.g. non-Win32
    /// platforms) or isn't a Win32 handle.
    fn set_taskbar_button_visible(frame: &eframe::Frame, visible: bool) {
        let Ok(handle) = frame.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        let hwnd = win32.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        unsafe {
            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if visible {
                ex_style &= !(WS_EX_TOOLWINDOW as isize);
            } else {
                ex_style |= WS_EX_TOOLWINDOW as isize;
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);
            // WS_EX_TOOLWINDOW changes don't reliably take effect on the
            // taskbar until the frame is explicitly refreshed.
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
        }
    }

    pub fn refresh_monitors(&mut self) {
        self.monitors = get_monitors();
        if self.config.monitor_index >= self.monitors.len() {
            self.config.monitor_index = 0;
            let _ = self.config.save();
        }
    }

    /// Switches the active capture profile: writes the current live
    /// settings back into whichever profile was active before switching
    /// (so nothing typed is lost), then loads `target_id`'s saved values
    /// into the live config. `do_capture()`/`start_video()`/etc. keep
    /// reading the same flat `AppConfig` fields as always — only *which*
    /// profile's values are currently loaded into them changes.
    pub fn switch_to_profile(&mut self, target_id: &str) {
        if self.profiles.active_profile_id.as_deref() == Some(target_id) {
            return;
        }
        if let Some(active) = self.profiles.active_mut() {
            active.update_from_config(&self.config);
        }
        let _ = self.profiles.save();

        let Some(target) = self.profiles.find_mut(target_id) else {
            self.status_message = format!("Profile not found: {target_id}");
            return;
        };
        target.write_into_config(&mut self.config);
        let target_name = target.name.clone();

        self.profiles.active_profile_id = Some(target_id.to_string());
        let _ = self.profiles.save();
        let _ = self.config.save();
        self.status_message = format!("📂 Switched to profile: {target_name}");
    }

    /// Creates a new profile snapshotting the *current* live settings (so
    /// if you're already looking at what you want, naming it is all that's
    /// needed), then switches to it.
    pub fn create_profile(&mut self, name: String, kind: ProfileKind) {
        let name = if name.trim().is_empty() { "New Profile".to_string() } else { name.trim().to_string() };
        let id = self.profiles.unique_id_from_name(&name);
        let monitor_name = self.monitors.get(self.config.monitor_index).map(|m| m.name.clone()).unwrap_or_default();

        // Write back the currently-active profile first, same as a normal
        // switch, before adding and switching to the new one.
        if let Some(active) = self.profiles.active_mut() {
            active.update_from_config(&self.config);
        }
        let profile = CaptureProfile::from_config(&self.config, id.clone(), name.clone(), kind, monitor_name);
        self.profiles.profiles.push(profile);
        self.profiles.active_profile_id = Some(id.clone());
        let _ = self.profiles.save();
        self.profile_editing_id = Some(id);
        self.status_message = format!("📂 Created profile: {name}");
    }

    /// Duplicates an existing profile (new id/name, same settings) without
    /// switching to it — opens the editor on the new copy so it can be
    /// tweaked immediately.
    pub fn duplicate_profile(&mut self, source_id: &str) {
        let Some(source) = self.profiles.find(source_id) else {
            return;
        };
        let mut copy = source.clone();
        copy.name = format!("{} (copy)", source.name);
        copy.id = self.profiles.unique_id_from_name(&copy.name);
        let new_id = copy.id.clone();
        self.profiles.profiles.push(copy);
        let _ = self.profiles.save();
        self.profile_editing_id = Some(new_id);
    }

    /// Deletes a profile. Refuses to delete the last remaining one (a
    /// profile-less app has nowhere to keep the current settings tracked).
    /// If the deleted profile was active, switches to whichever profile is
    /// now first in the list.
    pub fn delete_profile(&mut self, id: &str) {
        if self.profiles.profiles.len() <= 1 {
            self.status_message = "Can't delete the only remaining profile.".to_string();
            return;
        }
        let was_active = self.profiles.active_profile_id.as_deref() == Some(id);
        self.profiles.profiles.retain(|p| p.id != id);
        if self.profile_editing_id.as_deref() == Some(id) {
            self.profile_editing_id = None;
        }
        if was_active {
            self.profiles.active_profile_id = None;
            if let Some(first_id) = self.profiles.profiles.first().map(|p| p.id.clone()) {
                self.switch_to_profile(&first_id);
                return; // switch_to_profile already saves
            }
        }
        let _ = self.profiles.save();
    }

    pub fn apply_hotkeys(&mut self) {
        let cap_key = &AVAILABLE_KEYS[self.temp_capture_key_idx];
        self.config.capture_hotkey.vk_code = cap_key.vk_code;
        self.config.capture_hotkey.key_name = cap_key.name.to_string();

        let sess_key = &AVAILABLE_KEYS[self.temp_session_key_idx];
        self.config.new_session_hotkey.vk_code = sess_key.vk_code;
        self.config.new_session_hotkey.key_name = sess_key.name.to_string();

        let vid_start_key = &AVAILABLE_KEYS[self.temp_video_start_key_idx];
        self.config.video_start_hotkey.vk_code = vid_start_key.vk_code;
        self.config.video_start_hotkey.key_name = vid_start_key.name.to_string();

        let vid_stop_key = &AVAILABLE_KEYS[self.temp_video_stop_key_idx];
        self.config.video_stop_hotkey.vk_code = vid_stop_key.vk_code;
        self.config.video_stop_hotkey.key_name = vid_stop_key.name.to_string();

        let toggle_key = &AVAILABLE_KEYS[self.temp_toggle_window_key_idx];
        self.config.toggle_window_hotkey.vk_code = toggle_key.vk_code;
        self.config.toggle_window_hotkey.key_name = toggle_key.name.to_string();

        let quick_capture_key = &AVAILABLE_KEYS[self.temp_quick_capture_key_idx];
        self.config.quick_capture_hotkey.vk_code = quick_capture_key.vk_code;
        self.config.quick_capture_hotkey.key_name = quick_capture_key.name.to_string();

        let _ = self.config.save();

        self.hotkey_manager.update_hotkeys(
            self.config.capture_hotkey.clone(),
            self.config.new_session_hotkey.clone(),
            self.config.video_start_hotkey.clone(),
            self.config.video_stop_hotkey.clone(),
            self.config.toggle_window_hotkey.clone(),
            self.config.quick_capture_hotkey.clone(),
        );

        self.status_message = format!(
            "⌨️ Updated Hotkeys: Capture = [{}], New Session = [{}]",
            self.config.capture_hotkey.display_string(),
            self.config.new_session_hotkey.display_string()
        );
    }

    fn open_folder(path: &Path) {
        let _ = Command::new("explorer").arg(path).spawn();
    }

    fn open_file(path: &Path) {
        let _ = Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn();
    }

    fn select_in_explorer(path: &Path) {
        let _ = Command::new("explorer")
            .args(["/select,", &path.to_string_lossy()])
            .spawn();
    }

    fn copy_image_to_clipboard(path: &Path) -> Result<(), String> {
        let img = image::open(path).map_err(|e| e.to_string())?;
        let rgba = img.to_rgba8();
        Self::copy_image_data_to_clipboard(&rgba)
    }

    /// Same as `copy_image_to_clipboard`, but from an in-memory image
    /// rather than round-tripping through disk — used for the annotate
    /// toolbar's "Copy", which already has the baked pixels in hand.
    fn copy_image_data_to_clipboard(rgba: &image::RgbaImage) -> Result<(), String> {
        let (w, h) = rgba.dimensions();
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let img_data = arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: std::borrow::Cow::Borrowed(rgba.as_raw()),
        };
        clipboard.set_image(img_data).map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl eframe::App for ShotgunApp {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        self.maybe_toggle_taskbar_for_minimize(ctx, frame);
        self.handle_close_request(ctx, frame);

        // 1. Process System Tray Events
        if let Some(tray) = &self.tray_handler {
            if let Some(action) = tray.check_events() {
                match action {
                    TrayAction::ToggleShowWindow => {
                        self.toggle_window_visibility(ctx);
                    }
                    TrayAction::TriggerCapture => {
                        self.do_capture();
                    }
                    TrayAction::TriggerNewSession => {
                        self.trigger_new_session(ctx);
                    }
                    TrayAction::ExitApp => {
                        if self.video_encoding || self.pdf_export_rx.is_some() {
                            self.pending_exit = true;
                            self.status_message = "⏳ Waiting for video encode / PDF export to finish before exiting...".to_string();
                        } else {
                            self.exit_confirmed = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    }
                }
            }
        }

        // 2. Process Incoming Hotkey Events
        while let Ok(event) = self.hotkey_rx.try_recv() {
            match event {
                HotkeyEvent::Triggered(HotkeyAction::Capture) => {
                    self.do_capture();
                }
                HotkeyEvent::Triggered(HotkeyAction::NewSession) => {
                    self.trigger_new_session(ctx);
                }
                HotkeyEvent::Triggered(HotkeyAction::VideoStart) => {
                    self.start_video();
                }
                HotkeyEvent::Triggered(HotkeyAction::VideoStop) => {
                    self.stop_video();
                }
                HotkeyEvent::Triggered(HotkeyAction::ToggleWindow) => {
                    self.toggle_window_visibility(ctx);
                }
                HotkeyEvent::Triggered(HotkeyAction::QuickCapture) => {
                    self.trigger_quick_region_capture(ctx);
                }
                HotkeyEvent::RegisteredStatus {
                    capture_ok,
                    new_session_ok,
                    video_start_ok,
                    video_stop_ok,
                    toggle_window_ok,
                    quick_capture_ok,
                    error_msg,
                } => {
                    self.hotkey_status = (capture_ok, new_session_ok, video_start_ok, video_stop_ok, toggle_window_ok, quick_capture_ok);
                    self.hotkey_error = error_msg;
                }
            }
        }

        // 3. Complete any drag-select overlay whose window decorations were
        //    just dropped last frame, now that the resize can actually stick.
        self.apply_pending_fullscreen_overlay(ctx);
        self.apply_pending_fullscreen_enter(ctx);
        self.apply_pending_restore_size(ctx);

        // Poll async work (video encode, PDF export) without ever blocking
        // the UI thread on either.
        self.poll_video_result();
        self.poll_pdf_export_result();

        // A close/exit was requested while one of those was in flight
        // (see `handle_close_request` / `TrayAction::ExitApp`) — now that
        // both have had a chance to finish, let it through.
        if self.pending_exit && !self.video_encoding && self.pdf_export_rx.is_none() {
            self.pending_exit = false;
            self.exit_confirmed = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        // 4. Render Full-Screen Snipping Overlay if Active
        match self.overlay.show(ctx) {
            OverlayAction::Confirmed(region) => {
                if let Some(profile_id) = self.profile_region_picker_target.take() {
                    // Came from the profile editor's own "Drag-Select
                    // Region" button — write into that profile specifically,
                    // not the live config, and restore whichever monitor
                    // was actually live before the picker borrowed it.
                    let monitor_index = self.config.monitor_index;
                    let is_active = self.profiles.active_profile_id.as_deref() == Some(profile_id.as_str());
                    if let Some(profile) = self.profiles.find_mut(&profile_id) {
                        profile.region = Some(region);
                        profile.monitor_index = monitor_index;
                        let _ = self.profiles.save();
                    }
                    let prev_monitor = self.profile_region_picker_prev_monitor.take();
                    if is_active {
                        // The active profile *is* the live config — carry
                        // the new region/monitor over (the picker already
                        // pointed `config.monitor_index` at it), rather
                        // than reverting to the pre-picker monitor.
                        self.config.region = Some(region);
                        self.config.monitor_index = monitor_index;
                        let _ = self.config.save();
                    } else if let Some(prev) = prev_monitor {
                        self.config.monitor_index = prev;
                    }
                    self.status_message = format!(
                        "🎯 Region set for profile: X={}, Y={}, {}x{} px",
                        region.x, region.y, region.width, region.height
                    );
                } else {
                    self.config.region = Some(region);
                    let _ = self.config.save();
                    self.status_message = format!(
                        "🎯 Region selected: X={}, Y={}, {}x{} px",
                        region.x, region.y, region.width, region.height
                    );
                }
                self.restore_window_after_overlay(ctx);
            }
            OverlayAction::Cancelled => {
                self.status_message = "Selection cancelled.".to_string();
                self.pending_quick_capture = false;
                if self.profile_region_picker_target.take().is_some() {
                    if let Some(prev) = self.profile_region_picker_prev_monitor.take() {
                        self.config.monitor_index = prev;
                    }
                }
                self.restore_window_after_overlay(ctx);
            }
            OverlayAction::Annotated { image, region, explicit_copy } => {
                self.config.region = Some(region);
                let _ = self.config.save();
                // Already handled via the annotate toolbar's Save/Copy —
                // don't also let the pending Quick Capture auto-fire a
                // plain do_capture() once the window finishes restoring.
                self.pending_quick_capture = false;
                self.finish_annotated_capture(image, explicit_copy);
                self.restore_window_after_overlay(ctx);
            }
            OverlayAction::None => {}
        }

        if self.overlay.is_active {
            return;
        }

        // 5. Top Header & Nav Bar (Compact & Sleek)
        TopBottomPanel::top("top_header").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🎯 shotGun").size(17.0).strong().color(Color32::from_rgb(0, 220, 255)));
                ui.label(RichText::new("v0.4.0").size(11.0).color(Color32::GRAY));

                ui.separator();

                // Hotkey status badge
                let (cap_ok, sess_ok, vid_start_ok, vid_stop_ok, toggle_ok, quick_capture_ok) = self.hotkey_status;
                if cap_ok && sess_ok && vid_start_ok && vid_stop_ok && toggle_ok && quick_capture_ok {
                    ui.label(RichText::new("🟢 Active").size(11.0).color(Color32::from_rgb(70, 220, 100)));
                } else {
                    let err = self.hotkey_error.as_deref().unwrap_or("Binding error");
                    ui.label(RichText::new(format!("🔴 {err}")).size(11.0).color(Color32::from_rgb(255, 100, 100)));
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new("🔄 New Session").size(12.0).strong()).on_hover_text(format!("Reset counter & configure session. Hotkey: [{}]", self.config.new_session_hotkey.display_string())).clicked() {
                        self.trigger_new_session(ctx);
                    }

                    if ui.button(RichText::new("📸 Capture").size(12.0).color(Color32::from_rgb(0, 220, 255)).strong()).on_hover_text(format!("Capture selected region. Hotkey: [{}]", self.config.capture_hotkey.display_string())).clicked() {
                        self.do_capture();
                    }

                    if self.video_handle.is_some() {
                        if ui.button(RichText::new("⏹ Stop Video").size(12.0).color(Color32::from_rgb(255, 100, 100)).strong()).on_hover_text(format!("Stop video recording. Hotkey: [{}]", self.config.video_stop_hotkey.display_string())).clicked() {
                            self.stop_video();
                        }
                        if let Some(started_at) = self.recording_started_at {
                            let elapsed = started_at.elapsed().as_secs();
                            // Pulses by alternating the dot's brightness twice a second.
                            let pulse_on = (ctx.input(|i| i.time) * 2.0) as u64 % 2 == 0;
                            let dot = if pulse_on { "🔴" } else { "⭕" };
                            ui.label(
                                RichText::new(format!("{dot} REC {:02}:{:02}:{:02}", elapsed / 3600, (elapsed / 60) % 60, elapsed % 60))
                                    .size(12.0)
                                    .strong()
                                    .color(Color32::from_rgb(255, 100, 100)),
                            );
                        }
                    } else if self.video_encoding {
                        ui.add_enabled(false, Button::new(RichText::new("⏺ Record Video").size(12.0)));
                        ui.spinner();
                    } else if ui.button(RichText::new("⏺ Record Video").size(12.0).color(Color32::from_rgb(255, 180, 0)).strong()).on_hover_text(format!("Start video recording. Hotkey: [{}]", self.config.video_start_hotkey.display_string())).clicked() {
                        self.start_video();
                    }
                });
            });
            ui.add_space(3.0);

            // Tab Navigation Bar
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, ActiveTab::ScreenRegion, "🖥️ Screen");
                ui.selectable_value(&mut self.active_tab, ActiveTab::Hotkeys, "⌨️ Hotkeys");
                ui.selectable_value(&mut self.active_tab, ActiveTab::OutputNaming, "📁 Output");
                ui.selectable_value(&mut self.active_tab, ActiveTab::History, format!("📜 History ({})", self.history.len()));
                ui.selectable_value(&mut self.active_tab, ActiveTab::Settings, "⚙️ Settings");
                ui.selectable_value(&mut self.active_tab, ActiveTab::About, "ℹ️ About");
            });
            ui.add_space(2.0);
        });

        // 6. Bottom Status Bar
        TopBottomPanel::bottom("bottom_status").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if self.video_encoding || self.pdf_export_rx.is_some() {
                    ui.spinner();
                }
                ui.label(RichText::new(&self.status_message).size(11.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mon_name = self
                        .monitors
                        .get(self.config.monitor_index)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| "Display 1".to_string());
                    let reg_str = match self.config.region {
                        Some(r) => format!("ROI: {}x{} @ ({},{})", r.width, r.height, r.x, r.y),
                        None => "Full Screen".to_string(),
                    };
                    ui.label(RichText::new(format!("{mon_name} | {reg_str}")).size(11.0).color(Color32::GRAY));
                    if let Some(active) = self.profiles.active() {
                        ui.label(RichText::new(format!("📂 {}", active.name)).size(11.0).color(Color32::from_rgb(0, 220, 255)));
                        ui.separator();
                    }
                });
            });
            ui.add_space(2.0);
        });

        // 7. Central Content Panel
        CentralPanel::default().show(ctx, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.active_tab {
                    ActiveTab::ScreenRegion => self.render_screen_region_tab(ui, ctx),
                    ActiveTab::Hotkeys => self.render_hotkeys_tab(ui),
                    ActiveTab::OutputNaming => self.render_output_tab(ui),
                    ActiveTab::History => self.render_history_tab(ui),
                    ActiveTab::Settings => self.render_settings_tab(ui),
                    ActiveTab::About => self.render_about_tab(ui),
                }
            });
        });

        // 8. Manage Profiles Window
        self.render_profile_manager_window(ctx);

        // 9. Interactive Session Reset Modal
        if self.session_modal_open {
            egui::Window::new("🔄 Configure New Session")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([460.0, 240.0])
                .show(ctx, |ui| {
                    ui.label("Configure destination folder and numbering for this new session:");
                    ui.add_space(8.0);

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Folder:");
                            ui.label(RichText::new(self.temp_session_path.to_string_lossy()).monospace().size(11.0));
                            if ui.button("Browse...").clicked() {
                                if let Some(folder) = rfd::FileDialog::new()
                                    .set_directory(&self.temp_session_path)
                                    .pick_folder()
                                {
                                    self.temp_session_path = folder;
                                }
                            }
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("File Prefix:");
                            ui.text_edit_singleline(&mut self.temp_session_prefix);

                            ui.label("Start Count:");
                            ui.add(egui::DragValue::new(&mut self.temp_session_start_counter).range(0..=99999));
                        });

                        ui.add_space(4.0);
                        ui.checkbox(&mut self.temp_session_use_subfolder, "Create dedicated subfolder for session");
                    });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("🚀 Start Session").strong().color(Color32::from_rgb(0, 220, 255))).clicked() {
                            self.apply_new_session_from_modal();
                        }
                        if ui.button("Cancel").clicked() {
                            self.session_modal_open = false;
                        }
                    });
                });
        }
    }
}

impl ShotgunApp {
    /// The row of profile "pills" shown at the top of the Screen tab —
    /// click one to switch, "+ New" to create one from the current live
    /// settings, "⚙ Manage" to open the full editor.
    fn render_profile_pills(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("Profile:");
                let active_id = self.profiles.active_profile_id.clone();
                let mut clicked: Option<String> = None;
                for profile in &self.profiles.profiles {
                    let is_active = active_id.as_deref() == Some(profile.id.as_str());
                    let label = format!("{} {}", profile.kind.label(), profile.name);
                    if ui.selectable_label(is_active, label).clicked() {
                        clicked = Some(profile.id.clone());
                    }
                }
                if let Some(id) = clicked {
                    self.switch_to_profile(&id);
                }

                ui.separator();
                if ui.button("➕ New").on_hover_text("Save the current Screen/Output settings as a new profile, then open it to set its region, folder, and file pattern").clicked() {
                    self.create_profile(format!("Profile {}", self.profiles.profiles.len() + 1), ProfileKind::Screenshot);
                    // Open the editor on it straight away — the point of a
                    // new profile is to name it and set its region/folder/
                    // file pattern, not to silently get a copy of the
                    // current settings under a placeholder name.
                    self.profile_manager_open = true;
                }
                if ui.button("⚙ Manage").clicked() {
                    self.profile_manager_open = true;
                    if self.profile_editing_id.is_none() {
                        self.profile_editing_id = self.profiles.active_profile_id.clone();
                    }
                }
            });
        });
    }

    /// "⚙ Manage Profiles" window: a profile list (select/duplicate/delete,
    /// create new) plus the full field editor for whichever profile is
    /// currently selected in that list.
    fn render_profile_manager_window(&mut self, ctx: &Context) {
        if !self.profile_manager_open {
            return;
        }
        let mut open = true;

        egui::Window::new("🗂️ Manage Profiles")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([540.0, 520.0])
            .show(ctx, |ui| {
                ui.strong("Profiles:");
                ui.add_space(4.0);

                let active_id = self.profiles.active_profile_id.clone();
                let mut select_id: Option<String> = None;
                let mut duplicate_id: Option<String> = None;
                let mut delete_id: Option<String> = None;

                ScrollArea::vertical().max_height(140.0).id_salt("profile_manager_list").show(ui, |ui| {
                    for profile in &self.profiles.profiles {
                        ui.horizontal(|ui| {
                            let is_editing = self.profile_editing_id.as_deref() == Some(profile.id.as_str());
                            let is_active = active_id.as_deref() == Some(profile.id.as_str());
                            let label = format!("{}{} {}", if is_active { "🔘 " } else { "⚪ " }, profile.kind.label(), profile.name);
                            if ui.selectable_label(is_editing, label).clicked() {
                                select_id = Some(profile.id.clone());
                            }
                            if ui.small_button("⧉").on_hover_text("Duplicate").clicked() {
                                duplicate_id = Some(profile.id.clone());
                            }
                            if ui.small_button("🗑").on_hover_text("Delete (asks to confirm)").clicked() {
                                delete_id = Some(profile.id.clone());
                            }
                        });
                    }
                });
                if let Some(id) = select_id {
                    self.profile_editing_id = Some(id);
                }
                if let Some(id) = duplicate_id {
                    self.duplicate_profile(&id);
                }
                // Deleting is destructive — the trash button only arms a
                // confirmation; the actual delete needs a second click.
                if let Some(id) = delete_id {
                    self.profile_pending_delete = Some(id);
                }
                if let Some(pending_id) = self.profile_pending_delete.clone() {
                    let name = self.profiles.find(&pending_id).map(|p| p.name.clone()).unwrap_or_default();
                    let mut confirmed = false;
                    let mut cancelled = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Delete profile \"{name}\"?")).color(Color32::from_rgb(255, 120, 120)));
                        if ui.button("Yes, delete").clicked() {
                            confirmed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancelled = true;
                        }
                    });
                    if confirmed {
                        self.delete_profile(&pending_id);
                        self.profile_pending_delete = None;
                    } else if cancelled {
                        self.profile_pending_delete = None;
                    }
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("New profile name:");
                    ui.text_edit_singleline(&mut self.profile_new_name);
                    if ui.button("📸 New Screenshot Profile").clicked() {
                        let name = std::mem::take(&mut self.profile_new_name);
                        self.create_profile(name, ProfileKind::Screenshot);
                    }
                    if ui.button("🎬 New Video Profile").clicked() {
                        let name = std::mem::take(&mut self.profile_new_name);
                        self.create_profile(name, ProfileKind::Video);
                    }
                });

                ui.separator();

                if let Some(editing_id) = self.profile_editing_id.clone() {
                    self.render_profile_editor_fields(ui, ctx, &editing_id);
                } else {
                    ui.label(RichText::new("Select a profile above to edit it.").color(Color32::GRAY));
                }
            });

        self.profile_manager_open = open;
    }

    /// The actual field editor for one profile, used inside the Manage
    /// Profiles window. `self.monitors` is cloned up front so it can be
    /// read (for the monitor picker) at the same time `self.profiles` is
    /// mutably borrowed (to edit the profile's fields) — these are
    /// disjoint fields on `self`, but going through the `find_mut` method
    /// borrows all of `self.profiles`, so cloning the small monitor list
    /// sidesteps needing the borrow checker to see through that.
    fn render_profile_editor_fields(&mut self, ui: &mut Ui, ctx: &Context, editing_id: &str) {
        let monitors = self.monitors.clone();
        let mut want_drag_select = false;
        let is_active = self.profiles.active_profile_id.as_deref() == Some(editing_id);

        let Some(profile) = self.profiles.find_mut(editing_id) else {
            self.profile_editing_id = None;
            return;
        };
        // For the *active* profile, the live config is the source of truth
        // between edits (captures advance `counter`/`session_index` there,
        // not in the stored profile). Pull those in first, so the
        // write-through below at the end of this function can't roll them
        // back to whatever the profile last saw.
        if is_active {
            profile.update_from_config(&self.config);
        }
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.label("Name:");
            changed |= ui.text_edit_singleline(&mut profile.name).changed();
        });
        ui.label(format!("Type: {} (fixed at creation — duplicate to change)", profile.kind.label()));

        ui.add_space(6.0);
        ui.strong("Monitor:");
        for (idx, mon) in monitors.iter().enumerate() {
            let is_selected = profile.monitor_index == idx;
            let label = format!("{} {} - {}x{}", if is_selected { "🔘" } else { "⚪" }, mon.name, mon.width, mon.height);
            if ui.selectable_label(is_selected, label).clicked() {
                profile.monitor_index = idx;
                profile.monitor_name = mon.name.clone();
                changed = true;
            }
        }

        ui.add_space(6.0);
        ui.strong("Region:");
        ui.horizontal(|ui| {
            if ui.selectable_label(profile.region.is_none(), "🖥️ Full Screen").clicked() {
                profile.region = None;
                changed = true;
            }
            if ui.selectable_label(profile.region.is_some(), "✂️ Custom Region").clicked() && profile.region.is_none() {
                if let Some(mon) = monitors.get(profile.monitor_index) {
                    profile.region = Some(RectRegion { x: 0, y: 0, width: mon.width, height: mon.height });
                    changed = true;
                }
            }
            if ui.button("🎯 Drag-Select Region").on_hover_text("Freezes the profile's monitor to draw a precise region").clicked() {
                want_drag_select = true;
            }
        });
        if let Some(mut r) = profile.region {
            ui.horizontal(|ui| {
                ui.label("X:");
                changed |= ui.add(egui::DragValue::new(&mut r.x).range(0..=10000)).changed();
                ui.label("Y:");
                changed |= ui.add(egui::DragValue::new(&mut r.y).range(0..=10000)).changed();
                ui.label("W:");
                changed |= ui.add(egui::DragValue::new(&mut r.width).range(1..=10000)).changed();
                ui.label("H:");
                changed |= ui.add(egui::DragValue::new(&mut r.height).range(1..=10000)).changed();
            });
            profile.region = Some(r);
        }

        ui.add_space(6.0);
        ui.strong("Output:");
        ui.horizontal(|ui| {
            ui.label(RichText::new(profile.output_dir.to_string_lossy()).monospace().size(11.0));
            if ui.button("📂 Browse...").clicked() {
                if let Some(folder) = rfd::FileDialog::new().set_directory(&profile.output_dir).pick_folder() {
                    profile.output_dir = folder;
                    changed = true;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("File Prefix:");
            changed |= ui.text_edit_singleline(&mut profile.file_prefix).changed();
            ui.label("Digits:");
            changed |= ui.add(egui::DragValue::new(&mut profile.padding_digits).range(1..=8)).changed();
        });
        let preview_ext = match profile.kind {
            ProfileKind::Screenshot => profile.format.extension().to_string(),
            ProfileKind::Video => "mp4".to_string(),
        };
        ui.label(
            RichText::new(format!(
                "Next Output: {}{:0width$}.{}",
                profile.file_prefix, profile.counter, preview_ext, width = profile.padding_digits
            ))
            .color(Color32::GRAY)
            .size(10.5),
        );

        ui.add_space(6.0);
        match profile.kind {
            ProfileKind::Screenshot => {
                ui.strong("Format:");
                ui.horizontal(|ui| {
                    for fmt in OutputFormat::ALL {
                        if ui.selectable_value(&mut profile.format, fmt, fmt.extension().to_uppercase()).clicked() {
                            changed = true;
                        }
                    }
                });
                if profile.format == OutputFormat::Jpeg {
                    ui.horizontal(|ui| {
                        ui.label("Quality:");
                        changed |= ui.add(egui::Slider::new(&mut profile.jpeg_quality, 1..=100).text("%")).changed();
                    });
                }
            }
            ProfileKind::Video => {
                ui.strong("Video:");
                ui.horizontal(|ui| {
                    ui.label("Target FPS:");
                    changed |= ui.add(egui::DragValue::new(&mut profile.video_fps).range(1..=60)).changed();
                });
                changed |= ui.checkbox(&mut profile.cleanup_video_frames_after_encode, "Delete intermediate frames/audio after encode").changed();
            }
        }

        ui.add_space(6.0);
        changed |= ui.checkbox(&mut profile.auto_copy_to_clipboard, "Automatically copy to clipboard on capture").changed();
        changed |= ui.checkbox(&mut profile.auto_export_pdf_on_session, "Automatically bundle into a PDF on New Session").changed();

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Session Prefix:");
            changed |= ui.text_edit_singleline(&mut profile.session_prefix).changed();
            changed |= ui.checkbox(&mut profile.use_session_subfolders, "Use session subfolders").changed();
        });

        if changed {
            // Editing the *active* profile has to reach the live config too:
            // captures read `self.config`, not the stored profile, and the
            // next profile switch writes the live config back over the
            // profile — so without this, the edits would both do nothing
            // and then be silently overwritten and lost.
            if is_active {
                profile.write_into_config(&mut self.config);
                let _ = self.config.save();
            }
            let _ = self.profiles.save();
        }

        if want_drag_select {
            self.start_profile_region_picker(ctx, editing_id);
        }
    }

    fn render_screen_region_tab(&mut self, ui: &mut Ui, ctx: &Context) {
        self.render_profile_pills(ui);
        ui.add_space(6.0);

        ui.heading("🖥️ Display & Region of Interest (ROI)");
        ui.add_space(4.0);

        // Monitor Selector Card
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong("Target Monitor:");
                if ui.button("🔄 Refresh").clicked() {
                    self.refresh_monitors();
                }
            });
            ui.add_space(2.0);

            if self.monitors.is_empty() {
                ui.colored_label(Color32::from_rgb(255, 100, 100), "No monitors detected!");
            } else {
                for (idx, mon) in self.monitors.iter().enumerate() {
                    let is_selected = self.config.monitor_index == idx;
                    let primary_badge = if mon.is_primary { " [Primary]" } else { "" };
                    let label_text = format!(
                        "{} {}{} - {}x{} (scale: {:.1}x)",
                        if is_selected { "🔘" } else { "⚪" },
                        mon.name,
                        primary_badge,
                        mon.width,
                        mon.height,
                        mon.scale_factor,
                    );

                    if ui.selectable_label(is_selected, label_text).clicked() {
                        self.config.monitor_index = idx;
                        let _ = self.config.save();
                    }
                }
            }
        });

        ui.add_space(6.0);

        // Region Selection Mode Card
        ui.group(|ui| {
            ui.strong("Capture Area (ROI):");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let is_full = self.config.region.is_none();
                if ui.selectable_label(is_full, "🖥️ Full Screen").clicked() {
                    self.config.region = None;
                    let _ = self.config.save();
                    self.status_message = "Mode: Full Screen.".to_string();
                }

                let is_custom = self.config.region.is_some();
                if ui.selectable_label(is_custom, "✂️ Custom Region Box").clicked() {
                    if self.config.region.is_none() {
                        if let Some(mon) = self.monitors.get(self.config.monitor_index) {
                            self.config.region = Some(RectRegion {
                                x: 0,
                                y: 0,
                                width: mon.width,
                                height: mon.height,
                            });
                            let _ = self.config.save();
                        }
                    }
                }
            });

            ui.add_space(6.0);

            // Big Interactive Drag Button
            if ui.add_sized(
                [ui.available_width(), 32.0],
                Button::new(RichText::new("🎯 Drag-Select ROI on Screen (Snipping Overlay)").size(13.5).strong().color(Color32::from_rgb(0, 220, 255)))
            ).on_hover_text("Freezes screen to draw a precise ROI rectangle").clicked() {
                self.start_drag_select_overlay(ctx);
            }

            ui.add_space(6.0);

            // Quick presets
            ui.horizontal_wrapped(|ui| {
                ui.label("Presets:");
                if let Some(mon) = self.monitors.get(self.config.monitor_index) {
                    let w = mon.width;
                    let h = mon.height;

                    if ui.button("Top 50%").clicked() {
                        self.config.region = Some(RectRegion { x: 0, y: 0, width: w, height: h / 2 });
                        let _ = self.config.save();
                    }
                    if ui.button("Bottom 50%").clicked() {
                        self.config.region = Some(RectRegion { x: 0, y: h / 2, width: w, height: h / 2 });
                        let _ = self.config.save();
                    }
                    if ui.button("Left 50%").clicked() {
                        self.config.region = Some(RectRegion { x: 0, y: 0, width: w / 2, height: h });
                        let _ = self.config.save();
                    }
                    if ui.button("Right 50%").clicked() {
                        self.config.region = Some(RectRegion { x: w / 2, y: 0, width: w / 2, height: h });
                        let _ = self.config.save();
                    }
                    if ui.button("Center 50%").clicked() {
                        self.config.region = Some(RectRegion { x: w / 4, y: h / 4, width: w / 2, height: h / 2 });
                        let _ = self.config.save();
                    }
                }
            });

            // Manual coordinate fields
            if let Some(mut r) = self.config.region {
                ui.add_space(4.0);
                ui.separator();
                ui.strong("Fine-tune Pixel Coordinates:");
                ui.add_space(2.0);

                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("X:");
                    changed |= ui.add(egui::DragValue::new(&mut r.x).speed(1.0).range(0..=10000)).changed();

                    ui.label("Y:");
                    changed |= ui.add(egui::DragValue::new(&mut r.y).speed(1.0).range(0..=10000)).changed();

                    ui.label("W:");
                    changed |= ui.add(egui::DragValue::new(&mut r.width).speed(1.0).range(1..=10000)).changed();

                    ui.label("H:");
                    changed |= ui.add(egui::DragValue::new(&mut r.height).speed(1.0).range(1..=10000)).changed();
                });

                if changed {
                    self.config.region = Some(r);
                    let _ = self.config.save();
                }
            }
        });
    }

    fn render_hotkeys_tab(&mut self, ui: &mut Ui) {
        ui.heading("⌨️ Global OS Hotkeys");
        ui.label("Works globally across Windows in any app or fullscreen game.");
        ui.add_space(6.0);

        // Capture Hotkey Box
        ui.group(|ui| {
            ui.strong(RichText::new("📸 Screenshot Capture Hotkey").size(14.0));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.config.capture_hotkey.ctrl, "Ctrl");
                ui.checkbox(&mut self.config.capture_hotkey.alt, "Alt");
                ui.checkbox(&mut self.config.capture_hotkey.shift, "Shift");
                ui.checkbox(&mut self.config.capture_hotkey.win, "Win");

                ui.separator();
                ui.label("Key:");
                let current_key_name = AVAILABLE_KEYS[self.temp_capture_key_idx].name;
                egui::ComboBox::from_id_salt("combo_capture_key")
                    .selected_text(current_key_name)
                    .show_ui(ui, |ui| {
                        for (idx, key) in AVAILABLE_KEYS.iter().enumerate() {
                            ui.selectable_value(&mut self.temp_capture_key_idx, idx, key.name);
                        }
                    });
            });

            let cap_str = {
                let mut temp = self.config.capture_hotkey.clone();
                temp.key_name = AVAILABLE_KEYS[self.temp_capture_key_idx].name.to_string();
                temp.display_string()
            };
            ui.label(RichText::new(format!("Current: [{cap_str}]")).strong().color(Color32::from_rgb(0, 220, 255)));
        });

        ui.add_space(8.0);

        // New Session / Reset Hotkey Box
        ui.group(|ui| {
            ui.strong(RichText::new("🔄 New Session / Reset Counter Hotkey").size(14.0));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.config.new_session_hotkey.ctrl, "Ctrl");
                ui.checkbox(&mut self.config.new_session_hotkey.alt, "Alt");
                ui.checkbox(&mut self.config.new_session_hotkey.shift, "Shift");
                ui.checkbox(&mut self.config.new_session_hotkey.win, "Win");

                ui.separator();
                ui.label("Key:");
                let current_sess_key = AVAILABLE_KEYS[self.temp_session_key_idx].name;
                egui::ComboBox::from_id_salt("combo_session_key")
                    .selected_text(current_sess_key)
                    .show_ui(ui, |ui| {
                        for (idx, key) in AVAILABLE_KEYS.iter().enumerate() {
                            ui.selectable_value(&mut self.temp_session_key_idx, idx, key.name);
                        }
                    });
            });

            let sess_str = {
                let mut temp = self.config.new_session_hotkey.clone();
                temp.key_name = AVAILABLE_KEYS[self.temp_session_key_idx].name.to_string();
                temp.display_string()
            };
            ui.label(RichText::new(format!("Current: [{sess_str}]")).strong().color(Color32::from_rgb(0, 220, 255)));
        });

        ui.add_space(8.0);

        // Video Start Hotkey Box
        ui.group(|ui| {
            ui.strong(RichText::new("⏺ Start Video Recording Hotkey").size(14.0));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.config.video_start_hotkey.ctrl, "Ctrl");
                ui.checkbox(&mut self.config.video_start_hotkey.alt, "Alt");
                ui.checkbox(&mut self.config.video_start_hotkey.shift, "Shift");
                ui.checkbox(&mut self.config.video_start_hotkey.win, "Win");

                ui.separator();
                ui.label("Key:");
                let current_vid_start_key = AVAILABLE_KEYS[self.temp_video_start_key_idx].name;
                egui::ComboBox::from_id_salt("combo_video_start_key")
                    .selected_text(current_vid_start_key)
                    .show_ui(ui, |ui| {
                        for (idx, key) in AVAILABLE_KEYS.iter().enumerate() {
                            ui.selectable_value(&mut self.temp_video_start_key_idx, idx, key.name);
                        }
                    });
            });

            let vid_start_str = {
                let mut temp = self.config.video_start_hotkey.clone();
                temp.key_name = AVAILABLE_KEYS[self.temp_video_start_key_idx].name.to_string();
                temp.display_string()
            };
            ui.label(RichText::new(format!("Current: [{vid_start_str}]")).strong().color(Color32::from_rgb(0, 220, 255)));
        });

        ui.add_space(8.0);

        // Video Stop Hotkey Box
        ui.group(|ui| {
            ui.strong(RichText::new("⏹ Stop Video Recording Hotkey").size(14.0));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.config.video_stop_hotkey.ctrl, "Ctrl");
                ui.checkbox(&mut self.config.video_stop_hotkey.alt, "Alt");
                ui.checkbox(&mut self.config.video_stop_hotkey.shift, "Shift");
                ui.checkbox(&mut self.config.video_stop_hotkey.win, "Win");

                ui.separator();
                ui.label("Key:");
                let current_vid_stop_key = AVAILABLE_KEYS[self.temp_video_stop_key_idx].name;
                egui::ComboBox::from_id_salt("combo_video_stop_key")
                    .selected_text(current_vid_stop_key)
                    .show_ui(ui, |ui| {
                        for (idx, key) in AVAILABLE_KEYS.iter().enumerate() {
                            ui.selectable_value(&mut self.temp_video_stop_key_idx, idx, key.name);
                        }
                    });
            });

            let vid_stop_str = {
                let mut temp = self.config.video_stop_hotkey.clone();
                temp.key_name = AVAILABLE_KEYS[self.temp_video_stop_key_idx].name.to_string();
                temp.display_string()
            };
            ui.label(RichText::new(format!("Current: [{vid_stop_str}]")).strong().color(Color32::from_rgb(0, 220, 255)));
        });

        ui.add_space(8.0);

        // Show/Hide Window Hotkey Box
        ui.group(|ui| {
            ui.strong(RichText::new("🪟 Show/Hide Window Hotkey").size(14.0));
            ui.label(RichText::new("Use this if the window ever gets hidden and the tray icon isn't reachable.").size(10.5).color(Color32::GRAY));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.config.toggle_window_hotkey.ctrl, "Ctrl");
                ui.checkbox(&mut self.config.toggle_window_hotkey.alt, "Alt");
                ui.checkbox(&mut self.config.toggle_window_hotkey.shift, "Shift");
                ui.checkbox(&mut self.config.toggle_window_hotkey.win, "Win");

                ui.separator();
                ui.label("Key:");
                let current_toggle_key = AVAILABLE_KEYS[self.temp_toggle_window_key_idx].name;
                egui::ComboBox::from_id_salt("combo_toggle_window_key")
                    .selected_text(current_toggle_key)
                    .show_ui(ui, |ui| {
                        for (idx, key) in AVAILABLE_KEYS.iter().enumerate() {
                            ui.selectable_value(&mut self.temp_toggle_window_key_idx, idx, key.name);
                        }
                    });
            });

            let toggle_str = {
                let mut temp = self.config.toggle_window_hotkey.clone();
                temp.key_name = AVAILABLE_KEYS[self.temp_toggle_window_key_idx].name.to_string();
                temp.display_string()
            };
            ui.label(RichText::new(format!("Current: [{toggle_str}]")).strong().color(Color32::from_rgb(0, 220, 255)));
        });

        ui.add_space(8.0);

        // Quick Region Capture Hotkey Box
        ui.group(|ui| {
            ui.strong(RichText::new("🎯 Quick Region Capture Hotkey").size(14.0));
            ui.label(RichText::new("Opens the drag-select overlay on whichever monitor your mouse is currently on, then captures immediately once you release the drag — no separate Capture press needed.").size(10.5).color(Color32::GRAY));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.config.quick_capture_hotkey.ctrl, "Ctrl");
                ui.checkbox(&mut self.config.quick_capture_hotkey.alt, "Alt");
                ui.checkbox(&mut self.config.quick_capture_hotkey.shift, "Shift");
                ui.checkbox(&mut self.config.quick_capture_hotkey.win, "Win");

                ui.separator();
                ui.label("Key:");
                let current_quick_capture_key = AVAILABLE_KEYS[self.temp_quick_capture_key_idx].name;
                egui::ComboBox::from_id_salt("combo_quick_capture_key")
                    .selected_text(current_quick_capture_key)
                    .show_ui(ui, |ui| {
                        for (idx, key) in AVAILABLE_KEYS.iter().enumerate() {
                            ui.selectable_value(&mut self.temp_quick_capture_key_idx, idx, key.name);
                        }
                    });
            });

            let quick_capture_str = {
                let mut temp = self.config.quick_capture_hotkey.clone();
                temp.key_name = AVAILABLE_KEYS[self.temp_quick_capture_key_idx].name.to_string();
                temp.display_string()
            };
            ui.label(RichText::new(format!("Current: [{quick_capture_str}]")).strong().color(Color32::from_rgb(0, 220, 255)));
        });

        ui.add_space(10.0);

        if ui.add_sized([ui.available_width(), 32.0], Button::new(RichText::new("💾 Save & Apply Hotkey Changes").strong())).clicked() {
            self.apply_hotkeys();
        }
    }

    fn render_output_tab(&mut self, ui: &mut Ui) {
        ui.heading("📁 Output Destination & Naming");
        ui.add_space(4.0);

        // Destination Folder Card
        ui.group(|ui| {
            ui.strong("Destination Folder:");
            ui.add_space(2.0);
            ui.label(RichText::new(self.config.output_dir.to_string_lossy()).monospace().size(11.5));
            ui.horizontal(|ui| {
                if ui.button("📂 Browse...").clicked() {
                    if let Some(folder) = rfd::FileDialog::new()
                        .set_directory(&self.config.output_dir)
                        .pick_folder()
                    {
                        self.config.output_dir = folder;
                        let _ = self.config.save();
                    }
                }
                if ui.button("🧭 Open Explorer").clicked() {
                    Self::open_folder(&self.config.output_dir);
                }
            });
        });

        ui.add_space(6.0);

        // Format & Quality Card
        ui.group(|ui| {
            ui.strong("Image Format:");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for fmt in OutputFormat::ALL {
                    if ui.selectable_value(&mut self.config.format, fmt, fmt.extension().to_uppercase()).clicked() {
                        let _ = self.config.save();
                    }
                }
            });

            if self.config.format == OutputFormat::Jpeg {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Quality:");
                    if ui.add(egui::Slider::new(&mut self.config.jpeg_quality, 1..=100).text("%")).changed() {
                        let _ = self.config.save();
                    }
                });
            }
        });

        ui.add_space(6.0);

        // Sequential Naming & Page 0 Card
        ui.group(|ui| {
            ui.strong("Sequential Numbering & Page 0 Options:");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Prefix:");
                if ui.text_edit_singleline(&mut self.config.file_prefix).changed() {
                    let _ = self.config.save();
                }

                ui.label("Digits:");
                if ui.add(egui::DragValue::new(&mut self.config.padding_digits).range(1..=8)).changed() {
                    let _ = self.config.save();
                }
            });

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Default Start Index:");
                let is_zero = self.config.start_index == 0;
                if ui.selectable_label(is_zero, "0 (Page 0)").clicked() {
                    self.config.start_index = 0;
                    let _ = self.config.save();
                }
                let is_one = self.config.start_index == 1;
                if ui.selectable_label(is_one, "1 (Page 1)").clicked() {
                    self.config.start_index = 1;
                    let _ = self.config.save();
                }
            });

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Current Counter:");
                if ui.add(egui::DragValue::new(&mut self.config.counter).range(0..=999999)).changed() {
                    let _ = self.config.save();
                }

                if ui.button("Reset to 0").clicked() {
                    self.config.counter = 0;
                    let _ = self.config.save();
                }

                if ui.button("Reset to 1").clicked() {
                    self.config.counter = 1;
                    let _ = self.config.save();
                }
            });

            ui.add_space(4.0);

            let preview_name = format!(
                "{}{:0width$}.{}",
                self.config.file_prefix,
                self.config.counter,
                self.config.format.extension(),
                width = self.config.padding_digits
            );
            ui.label(RichText::new(format!("Next Output: {preview_name}")).strong().color(Color32::from_rgb(0, 220, 255)));

            ui.add_space(4.0);
            ui.separator();

            if ui.checkbox(&mut self.config.prompt_on_new_session, "Prompt for Path and Name when starting New Session").changed() {
                let _ = self.config.save();
            }

            if ui.checkbox(&mut self.config.auto_increment, "Auto-increment number on each capture").changed() {
                let _ = self.config.save();
            }

            if ui.checkbox(&mut self.config.overwrite_existing, "Overwrite if file already exists").changed() {
                let _ = self.config.save();
            }
        });
    }

    fn render_settings_tab(&mut self, ui: &mut Ui) {
        ui.heading("⚙️ System & Behavior Settings");
        ui.add_space(4.0);

        ui.group(|ui| {
            ui.strong("Windows Integration:");
            ui.add_space(4.0);

            let mut autostart = self.autostart_state;
            if ui.checkbox(&mut autostart, "🚀 Launch shotGun automatically on Windows startup").changed() {
                if let Err(e) = set_autostart(autostart) {
                    self.status_message = format!("Autostart error: {e}");
                } else {
                    self.autostart_state = autostart;
                    self.status_message = if autostart {
                        "Windows Startup Autostart enabled.".to_string()
                    } else {
                        "Windows Startup Autostart disabled.".to_string()
                    };
                }
            }

            ui.add_space(4.0);
            if ui.checkbox(&mut self.config.minimize_to_tray, "Keep running in System Tray when minimized").changed() {
                let _ = self.config.save();
            }

            if ui.checkbox(&mut self.config.close_to_tray, "Closing the window (✕) also goes to tray, instead of exiting").changed() {
                let _ = self.config.save();
            }
            ui.label(
                RichText::new("Separate from the setting above: this changes what the title bar's ✕ button does. Use the tray icon's \"Exit shotGun\" (or the Show/Hide hotkey, then Exit) to actually quit.")
                    .size(10.5)
                    .color(Color32::GRAY),
            );

            if ui.checkbox(&mut self.config.play_sound, "Play audio chime on capture").changed() {
                let _ = self.config.save();
            }

            if ui.checkbox(&mut self.config.auto_copy_to_clipboard, "Automatically copy each screenshot to clipboard on capture").changed() {
                let _ = self.config.save();
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.strong("Session Settings:");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Current Session Index:");
                if ui.add(egui::DragValue::new(&mut self.config.session_index).range(1..=99999)).changed() {
                    let _ = self.config.save();
                }
            });

            if ui.checkbox(&mut self.config.use_session_subfolders, "Create dedicated subfolder for each session (e.g. session_01/)").changed() {
                let _ = self.config.save();
            }

            if ui.checkbox(&mut self.config.auto_export_pdf_on_session, "Automatically bundle a session's screenshots into a PDF when starting a New Session").changed() {
                let _ = self.config.save();
            }
            ui.label(
                RichText::new("Use this if you know a session is going to be multiple screenshots. The individual images are always kept either way — the PDF is additional. You can also export the current History tab to a PDF manually at any time.")
                    .size(10.5)
                    .color(Color32::GRAY),
            );
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.strong("Video Recording:");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Target FPS:");
                if ui.add(egui::DragValue::new(&mut self.config.video_fps).range(1..=60)).changed() {
                    let _ = self.config.save();
                }
            });

            ui.add_space(4.0);
            ui.label("ffmpeg Path (leave blank to auto-detect from app folder or system PATH):");
            ui.horizontal(|ui| {
                if ui.text_edit_singleline(&mut self.config.ffmpeg_path).changed() {
                    let _ = self.config.save();
                }
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ffmpeg executable", &["exe"])
                        .pick_file()
                    {
                        self.config.ffmpeg_path = path.to_string_lossy().to_string();
                        let _ = self.config.save();
                    }
                }
                if ui.button("Clear").clicked() {
                    self.config.ffmpeg_path.clear();
                    let _ = self.config.save();
                }
            });
            ui.label(
                RichText::new("Frames are always saved as PNGs first; ffmpeg is only used to mux them into an MP4 afterward. If ffmpeg can't be found, the PNG frames are kept.")
                    .size(10.5)
                    .color(Color32::GRAY),
            );

            ui.add_space(6.0);
            if ui.checkbox(&mut self.config.cleanup_video_frames_after_encode, "Delete PNG frames, audio.wav, and the concat script after the MP4 is created").changed() {
                let _ = self.config.save();
            }
            ui.label(
                RichText::new("Only the intermediate files for a successful encode are removed — output.mp4 is always kept, and frames are kept if encoding fails.")
                    .size(10.5)
                    .color(Color32::GRAY),
            );
        });
    }

    fn render_history_tab(&mut self, ui: &mut Ui) {
        ui.heading("📜 Capture History");
        ui.horizontal(|ui| {
            ui.label(format!("Total: {}", self.history.len()));
            if !self.history.is_empty() {
                if ui.button("📄 Export to PDF").clicked() {
                    self.export_history_to_pdf();
                }
                if ui.button("Clear").clicked() {
                    self.history.clear();
                }
            }
        });
        ui.add_space(6.0);

        if self.history.is_empty() {
            ui.label(RichText::new("No screenshots captured yet.").color(Color32::GRAY));
            return;
        }

        let mut remove_index: Option<usize> = None;

        for (index, item) in self.history.iter().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("#{:0width$}", item.counter, width = self.config.padding_digits));
                    ui.label(RichText::new(item.file_path.file_name().unwrap_or_default().to_string_lossy()).strong());
                    ui.label(format!("{}x{} px", item.width, item.height));
                    ui.label(format!("{:.1} KB", item.file_size_bytes as f64 / 1024.0));
                    ui.label(RichText::new(&item.timestamp).color(Color32::GRAY).size(11.0));
                });


                ui.horizontal(|ui| {
                    if ui.button("👁️ Open").clicked() {
                        Self::open_file(&item.file_path);
                    }
                    if ui.button("📂 Locate").clicked() {
                        Self::select_in_explorer(&item.file_path);
                    }
                    if ui.button("📋 Copy").clicked() {
                        if let Err(e) = Self::copy_image_to_clipboard(&item.file_path) {
                            self.status_message = format!("Clipboard error: {e}");
                        } else {
                            self.status_message = "Image copied to clipboard!".to_string();
                        }
                    }
                    if ui.button(RichText::new("🗑️ Remove").color(Color32::from_rgb(255, 120, 120))).on_hover_text("Remove from this list only — the saved file is kept.").clicked() {
                        remove_index = Some(index);
                    }
                });
            });
            ui.add_space(2.0);
        }

        if let Some(index) = remove_index {
            self.history.remove(index);
        }
    }

    fn render_about_tab(&mut self, ui: &mut Ui) {
        ui.heading("🎯 shotGun v0.4.0");
        ui.label(RichText::new("Created by Noerotech").size(14.0).strong().color(Color32::from_rgb(0, 220, 255)));
        ui.add_space(6.0);

        ui.label("High-performance screen & region capture utility built with Rust, native Win32 global hotkeys, immediate-mode GUI, and system tray integration.");
        ui.add_space(6.0);

        ui.group(|ui| {
            ui.strong("Features:");
            ui.label("• Multi-Monitor Detection & Selection");
            ui.label("• Interactive Freeze-Frame ROI Selector");
            ui.label("• Page 0 / 0-Indexed Sequential Numbering");
            ui.label("• Interactive Session Reset Prompt Dialog");
            ui.label("• System Tray Icon & Windows Startup Auto-Start");
            ui.label("• Global Hotkey Engine (Capture, New Session, Video Start/Stop, Show/Hide Window)");
            ui.label("• PNG, JPEG, BMP, and WebP Encoders");
            ui.label("• Video Recording (DXGI Desktop Duplication) with WASAPI Loopback Audio");
            ui.label("• Multi-Screenshot PDF Export (manual or auto per session)");
            ui.label("• Post-Capture Annotation Editor (Arrow, Rectangle, Highlighter, Text, Blur, Redact, Step Labels)");
            ui.label("• Capture Profiles: named, switchable region/output/naming setups");
            ui.label("• Compact & Responsive UI");
        });

        ui.add_space(6.0);
        ui.label(RichText::new("Repository: https://github.com/phonie-bytes/shotGun").color(Color32::from_rgb(100, 180, 255)));
    }
}

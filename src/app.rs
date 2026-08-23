use crate::autostart::{is_autostart_enabled, set_autostart};
use crate::capture::{
    execute_capture, get_monitors, CaptureResult, MonitorInfo,
};
use crate::config::{AppConfig, OutputFormat, RectRegion};
use crate::hotkey::{
    AVAILABLE_KEYS, HotkeyAction, HotkeyEvent, HotkeyManager,
};
use crate::overlay::{OverlayAction, RegionSelectorOverlay};
use crate::pdf_export;
use crate::tray::{TrayAction, TrayHandler};
use crate::video;
use crossbeam_channel::Receiver;
use egui::{
    Align, Button, CentralPanel, Color32, Context, Layout, RichText, ScrollArea, TopBottomPanel,
    Ui, ViewportCommand,
};

use std::path::{Path, PathBuf};
use std::process::Command;

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
    pub hotkey_status: (bool, bool, bool, bool, bool),
    pub hotkey_error: Option<String>,
    pub active_tab: ActiveTab,
    pub history: Vec<CaptureResult>,
    pub status_message: String,
    pub overlay: RegionSelectorOverlay,
    pub tray_handler: Option<TrayHandler>,
    pub window_visible: bool,
    pub autostart_state: bool,

    // Hotkey temp state
    pub temp_capture_key_idx: usize,
    pub temp_session_key_idx: usize,
    // Video hotkey temp state
    pub temp_video_start_key_idx: usize,
    pub temp_video_stop_key_idx: usize,
    // Show/Hide window hotkey temp state
    pub temp_toggle_window_key_idx: usize,
    // Video capture handle
    pub video_handle: Option<video::VideoHandle>,
    pub video_result_rx: Option<Receiver<video::VideoResult>>,

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

        // Initialize hotkey manager with all five hotkeys
        let (hotkey_manager, hotkey_rx) = HotkeyManager::new(
            config.capture_hotkey.clone(),
            config.new_session_hotkey.clone(),
            config.video_start_hotkey.clone(),
            config.video_stop_hotkey.clone(),
            config.toggle_window_hotkey.clone(),
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
            hotkey_status: (false, false, false, false, false),
            hotkey_error: None,
            active_tab: ActiveTab::ScreenRegion,
            history: Vec::new(),
            status_message: String::new(),
            overlay: RegionSelectorOverlay::new(),
            tray_handler,
            window_visible: true,
            autostart_state,
            // hotkey UI indices
            temp_capture_key_idx,
            temp_session_key_idx,
            temp_video_start_key_idx,
            temp_video_stop_key_idx,
            temp_toggle_window_key_idx,
            video_handle: None,
            video_result_rx: None,
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
                let msg = format!(
                    "📸 Captured #{:0width$} ({}x{}) -> {}",
                    result.counter,
                    result.width,
                    result.height,
                    result.file_path.file_name().unwrap_or_default().to_string_lossy(),
                    width = self.config.padding_digits
                );
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
        crate::video::debug_log("app.rs: start_video() called");
        let (handle, result_rx) = video::start_from_global(&self.config);
        self.video_handle = Some(handle);
        self.video_result_rx = Some(result_rx);
        self.status_message = "⏺️ Video recording started.".to_string();
    }

    pub fn stop_video(&mut self) {
        match self.video_handle.take() {
            Some(handle) => {
                crate::video::debug_log("app.rs: stop_video() called, stopping handle");
                self.status_message = "⏳ Stopping & encoding video...".to_string();
                handle.stop();
                if let Some(rx) = self.video_result_rx.take() {
                    match rx.recv() {
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
                        }
                        Err(_) => {
                            crate::video::debug_log("app.rs: result channel disconnected without a result (video thread likely panicked)");
                            self.status_message = "❌ Video capture failed unexpectedly (no result). Check shotgun_video_debug.log in %TEMP%.".to_string();
                        }
                    }
                }
            }
            None => {
                self.status_message = "No video recording in progress.".to_string();
            }
        }
    }

    pub fn trigger_new_session(&mut self, ctx: &Context) {
        if self.config.prompt_on_new_session {
            // Restore window if minimized and open prompt modal
            self.window_visible = true;
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
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
        match pdf_export::export_images_to_pdf(&items, &dest_path) {
            Ok(()) => {
                self.status_message = format!(
                    "{} 📄 Also exported {} screenshots to {}",
                    self.status_message,
                    items.len(),
                    dest_path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            Err(e) => {
                self.status_message = format!("{} ⚠️ PDF export failed: {e}", self.status_message);
            }
        }
    }

    /// Exports everything currently in the History tab to a single PDF
    /// (oldest capture first), letting the user pick the destination.
    /// Individual image files are never touched — this is purely additional.
    pub fn export_history_to_pdf(&mut self) {
        if self.history.is_empty() {
            self.status_message = "No screenshots in history to export.".to_string();
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

        match pdf_export::export_images_to_pdf(&items, &dest_path) {
            Ok(()) => {
                self.status_message = format!(
                    "📄 Exported {} screenshots to {}",
                    items.len(),
                    dest_path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            Err(e) => {
                self.status_message = format!("⚠️ PDF export failed: {e}");
            }
        }
    }

    pub fn toggle_window_visibility(&mut self, ctx: &Context) {
        self.window_visible = !self.window_visible;
        ctx.send_viewport_cmd(ViewportCommand::Visible(self.window_visible));
        if self.window_visible {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
    }

    pub fn refresh_monitors(&mut self) {
        self.monitors = get_monitors();
        if self.config.monitor_index >= self.monitors.len() {
            self.config.monitor_index = 0;
            let _ = self.config.save();
        }
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

        let _ = self.config.save();

        self.hotkey_manager.update_hotkeys(
            self.config.capture_hotkey.clone(),
            self.config.new_session_hotkey.clone(),
            self.config.video_start_hotkey.clone(),
            self.config.video_stop_hotkey.clone(),
            self.config.toggle_window_hotkey.clone(),
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
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));

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
                        ctx.send_viewport_cmd(ViewportCommand::Close);
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
                HotkeyEvent::RegisteredStatus {
                    capture_ok,
                    new_session_ok,
                    video_start_ok,
                    video_stop_ok,
                    toggle_window_ok,
                    error_msg,
                } => {
                    self.hotkey_status = (capture_ok, new_session_ok, video_start_ok, video_stop_ok, toggle_window_ok);
                    self.hotkey_error = error_msg;
                }
            }
        }

        // 3. Render Full-Screen Snipping Overlay if Active
        match self.overlay.show(ctx) {
            OverlayAction::Confirmed(region) => {
                self.config.region = Some(region);
                let _ = self.config.save();
                self.status_message = format!(
                    "🎯 Region selected: X={}, Y={}, {}x{} px",
                    region.x, region.y, region.width, region.height
                );
            }
            OverlayAction::Cancelled => {
                self.status_message = "Selection cancelled.".to_string();
            }
            OverlayAction::None => {}
        }

        if self.overlay.is_active {
            return;
        }

        // 4. Top Header & Nav Bar (Compact & Sleek)
        TopBottomPanel::top("top_header").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🎯 shotGun").size(17.0).strong().color(Color32::from_rgb(0, 220, 255)));
                ui.label(RichText::new("v0.3.0").size(11.0).color(Color32::GRAY));

                ui.separator();

                // Hotkey status badge
                let (cap_ok, sess_ok, vid_start_ok, vid_stop_ok, toggle_ok) = self.hotkey_status;
                if cap_ok && sess_ok && vid_start_ok && vid_stop_ok && toggle_ok {
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

        // 5. Bottom Status Bar
        TopBottomPanel::bottom("bottom_status").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
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
                });
            });
            ui.add_space(2.0);
        });

        // 6. Central Content Panel
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

        // 7. Interactive Session Reset Modal
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
    fn render_screen_region_tab(&mut self, ui: &mut Ui, ctx: &Context) {
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
                if let Err(e) = self.overlay.start(ctx, self.config.monitor_index) {
                    self.status_message = format!("Overlay error: {e}");
                }
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

            if ui.checkbox(&mut self.config.play_sound, "Play audio chime on capture").changed() {
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

        for item in &self.history {
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
                });
            });
            ui.add_space(2.0);
        }
    }

    fn render_about_tab(&mut self, ui: &mut Ui) {
        ui.heading("🎯 shotGun v0.3.0");
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
            ui.label("• Compact & Responsive UI");
        });

        ui.add_space(6.0);
        ui.label(RichText::new("Repository: https://github.com/phonie-bytes/shotGun").color(Color32::from_rgb(100, 180, 255)));
    }
}

use crate::capture::{
    execute_capture, get_monitors, CaptureResult, MonitorInfo,
};
use crate::config::{AppConfig, OutputFormat, RectRegion};
use crate::hotkey::{
    AVAILABLE_KEYS, HotkeyAction, HotkeyEvent, HotkeyManager,
};
use crate::overlay::{OverlayAction, RegionSelectorOverlay};
use crossbeam_channel::Receiver;
use egui::{
    Align, Button, CentralPanel, Color32, Context, Layout, RichText, ScrollArea,
    TopBottomPanel, Ui,
};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    ScreenRegion,
    Hotkeys,
    OutputNaming,
    History,
    About,
}

pub struct ShotgunApp {
    pub config: AppConfig,
    pub monitors: Vec<MonitorInfo>,
    pub hotkey_manager: HotkeyManager,
    pub hotkey_rx: Receiver<HotkeyEvent>,
    pub hotkey_status: (bool, bool),
    pub hotkey_error: Option<String>,
    pub active_tab: ActiveTab,
    pub history: Vec<CaptureResult>,
    pub status_message: String,
    pub overlay: RegionSelectorOverlay,
    // Hotkey edit state
    pub temp_capture_key_idx: usize,
    pub temp_session_key_idx: usize,
}

impl ShotgunApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.window_rounding = 8.0.into();
        visuals.panel_fill = Color32::from_rgb(20, 24, 30);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(28, 33, 41);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(36, 42, 53);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(52, 60, 75);
        visuals.widgets.active.bg_fill = Color32::from_rgb(68, 79, 99);
        cc.egui_ctx.set_visuals(visuals);

        let config = AppConfig::load();
        let monitors = get_monitors();

        let (hotkey_manager, hotkey_rx) = HotkeyManager::new(
            config.capture_hotkey.clone(),
            config.new_session_hotkey.clone(),
        );

        let temp_capture_key_idx = AVAILABLE_KEYS
            .iter()
            .position(|k| k.vk_code == config.capture_hotkey.vk_code)
            .unwrap_or(8); // Default F9

        let temp_session_key_idx = AVAILABLE_KEYS
            .iter()
            .position(|k| k.vk_code == config.new_session_hotkey.vk_code)
            .unwrap_or(9); // Default F10

        Self {
            config,
            monitors,
            hotkey_manager,
            hotkey_rx,
            hotkey_status: (true, true),
            hotkey_error: None,
            active_tab: ActiveTab::ScreenRegion,
            history: Vec::new(),
            status_message: "Ready. Press capture hotkey or button to take a screenshot.".to_string(),
            overlay: RegionSelectorOverlay::new(),
            temp_capture_key_idx,
            temp_session_key_idx,
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

    pub fn do_new_session(&mut self) {
        self.config.session_index += 1;
        self.config.counter = 1;
        let _ = self.config.save();
        self.status_message = format!(
            "🔄 Started New Session #{} (Counter reset to 1)",
            self.config.session_index
        );
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

        let _ = self.config.save();

        self.hotkey_manager.update_hotkeys(
            self.config.capture_hotkey.clone(),
            self.config.new_session_hotkey.clone(),
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
        // Request repaint if hotkey events or overlay active
        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        // Process incoming hotkey events
        while let Ok(event) = self.hotkey_rx.try_recv() {
            match event {
                HotkeyEvent::Triggered(HotkeyAction::Capture) => {
                    self.do_capture();
                }
                HotkeyEvent::Triggered(HotkeyAction::NewSession) => {
                    self.do_new_session();
                }
                HotkeyEvent::RegisteredStatus {
                    capture_ok,
                    new_session_ok,
                    error_msg,
                } => {
                    self.hotkey_status = (capture_ok, new_session_ok);
                    self.hotkey_error = error_msg;
                }
            }
        }

        // Render full-screen overlay if snipping mode is active
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

        // 1. Top Bar Header
        TopBottomPanel::top("top_header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🎯 shotGun").size(20.0).strong().color(Color32::from_rgb(0, 220, 255)));
                ui.label(RichText::new("v0.1.0").size(12.0).color(Color32::GRAY));

                ui.separator();

                // Hotkey status badge
                let (cap_ok, sess_ok) = self.hotkey_status;
                if cap_ok && sess_ok {
                    ui.label(RichText::new("🟢 Hotkeys Active").size(12.0).color(Color32::from_rgb(70, 220, 100)));
                } else {
                    let err = self.hotkey_error.as_deref().unwrap_or("Binding error");
                    ui.label(RichText::new(format!("🔴 {err}")).size(12.0).color(Color32::from_rgb(255, 100, 100)));
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new("🔄 New Session").strong()).on_hover_text(format!("Reset counter to 1 and advance session. Hotkey: [{}]", self.config.new_session_hotkey.display_string())).clicked() {
                        self.do_new_session();
                    }

                    if ui.button(RichText::new("📸 Capture Screen").color(Color32::from_rgb(0, 220, 255)).strong()).on_hover_text(format!("Capture selected region. Hotkey: [{}]", self.config.capture_hotkey.display_string())).clicked() {
                        self.do_capture();
                    }
                });
            });
            ui.add_space(4.0);

            // Tab bar
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, ActiveTab::ScreenRegion, "🖥️ Screen & Region");
                ui.selectable_value(&mut self.active_tab, ActiveTab::Hotkeys, "⌨️ Global Hotkeys");
                ui.selectable_value(&mut self.active_tab, ActiveTab::OutputNaming, "📁 Output & Naming");
                ui.selectable_value(&mut self.active_tab, ActiveTab::History, format!("📜 History ({})", self.history.len()));
                ui.selectable_value(&mut self.active_tab, ActiveTab::About, "ℹ️ About");
            });
            ui.add_space(4.0);
        });

        // 2. Bottom Status Bar
        TopBottomPanel::bottom("bottom_status").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status_message).size(13.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mon_name = self
                        .monitors
                        .get(self.config.monitor_index)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| "None".to_string());
                    let reg_str = match self.config.region {
                        Some(r) => format!("Region: {}x{} @ ({},{})", r.width, r.height, r.x, r.y),
                        None => "Full Screen".to_string(),
                    };
                    ui.label(RichText::new(format!("Monitor: {mon_name} | {reg_str}")).size(12.0).color(Color32::GRAY));
                });
            });
            ui.add_space(4.0);
        });

        // 3. Central Content Panel
        CentralPanel::default().show(ctx, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.active_tab {
                    ActiveTab::ScreenRegion => self.render_screen_region_tab(ui, ctx),
                    ActiveTab::Hotkeys => self.render_hotkeys_tab(ui),
                    ActiveTab::OutputNaming => self.render_output_tab(ui),
                    ActiveTab::History => self.render_history_tab(ui),
                    ActiveTab::About => self.render_about_tab(ui),
                }
            });
        });
    }
}

impl ShotgunApp {
    fn render_screen_region_tab(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.heading("🖥️ Display & Region Selection");
        ui.label("Choose which monitor to capture and specify the exact region of interest (ROI).");
        ui.add_space(8.0);

        // Monitor Selector Card
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong("Select Screen / Monitor:");
                if ui.button("🔄 Refresh Monitors").clicked() {
                    self.refresh_monitors();
                }
            });
            ui.add_space(4.0);

            if self.monitors.is_empty() {
                ui.colored_label(Color32::from_rgb(255, 100, 100), "No monitors detected!");
            } else {
                for (idx, mon) in self.monitors.iter().enumerate() {
                    let is_selected = self.config.monitor_index == idx;
                    let primary_badge = if mon.is_primary { " [Primary]" } else { "" };
                    let label_text = format!(
                        "{} {}{} - {}x{} (scale: {:.1}x) at ({}, {})",
                        if is_selected { "🔘" } else { "⚪" },
                        mon.name,
                        primary_badge,
                        mon.width,
                        mon.height,
                        mon.scale_factor,
                        mon.x,
                        mon.y
                    );

                    if ui.selectable_label(is_selected, label_text).clicked() {
                        self.config.monitor_index = idx;
                        let _ = self.config.save();
                    }
                }
            }
        });

        ui.add_space(12.0);

        // Region Selection Mode Card
        ui.group(|ui| {
            ui.strong("Region of Interest (ROI):");
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let is_full = self.config.region.is_none();
                if ui.selectable_label(is_full, "🖥️ Entire Screen (Full Monitor)").clicked() {
                    self.config.region = None;
                    let _ = self.config.save();
                    self.status_message = "Capture mode set to Full Screen.".to_string();
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

            ui.add_space(8.0);

            // Interactive Drag Selector Button
            if ui.add_sized(
                [ui.available_width(), 38.0],
                Button::new(RichText::new("🎯 Interactive Drag-Select on Screen (Snipping Overlay)").size(15.0).strong().color(Color32::from_rgb(0, 220, 255)))
            ).on_hover_text("Freezes the screen and opens an interactive rectangle selector tool").clicked() {
                if let Err(e) = self.overlay.start(ctx, self.config.monitor_index) {
                    self.status_message = format!("Failed to start snipping overlay: {e}");
                }
            }

            ui.add_space(8.0);

            // Quick presets
            ui.horizontal_wrapped(|ui| {
                ui.label("Quick Region Presets:");
                if let Some(mon) = self.monitors.get(self.config.monitor_index) {
                    let w = mon.width;
                    let h = mon.height;

                    if ui.button("Top Half").clicked() {
                        self.config.region = Some(RectRegion { x: 0, y: 0, width: w, height: h / 2 });
                        let _ = self.config.save();
                    }
                    if ui.button("Bottom Half").clicked() {
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
                ui.add_space(8.0);
                ui.separator();
                ui.strong("Fine-tune Pixel Coordinates:");
                ui.add_space(4.0);

                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("X:");
                    changed |= ui.add(egui::DragValue::new(&mut r.x).speed(1.0).range(0..=10000)).changed();

                    ui.label("Y:");
                    changed |= ui.add(egui::DragValue::new(&mut r.y).speed(1.0).range(0..=10000)).changed();

                    ui.label("Width:");
                    changed |= ui.add(egui::DragValue::new(&mut r.width).speed(1.0).range(1..=10000)).changed();

                    ui.label("Height:");
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
        ui.heading("⌨️ Global Hotkeys Configuration");
        ui.label("Global hotkeys work anywhere in Windows, even when shotGun is minimized or another application is active.");
        ui.add_space(8.0);

        // Capture Hotkey Box
        ui.group(|ui| {
            ui.strong(RichText::new("📸 Screenshot Capture Hotkey").size(15.0));
            ui.label("Triggers an instant screenshot of the selected screen / region.");
            ui.add_space(6.0);

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

            ui.add_space(4.0);
            let cap_str = {
                let mut temp = self.config.capture_hotkey.clone();
                temp.key_name = AVAILABLE_KEYS[self.temp_capture_key_idx].name.to_string();
                temp.display_string()
            };
            ui.label(RichText::new(format!("Current Binding: [{cap_str}]")).strong().color(Color32::from_rgb(0, 220, 255)));
        });

        ui.add_space(12.0);

        // New Session / Reset Hotkey Box
        ui.group(|ui| {
            ui.strong(RichText::new("🔄 Start New Session / Reset Counter Hotkey").size(15.0));
            ui.label("Resets the filename increment number back to 1 and starts a new session.");
            ui.add_space(6.0);

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

            ui.add_space(4.0);
            let sess_str = {
                let mut temp = self.config.new_session_hotkey.clone();
                temp.key_name = AVAILABLE_KEYS[self.temp_session_key_idx].name.to_string();
                temp.display_string()
            };
            ui.label(RichText::new(format!("Current Binding: [{sess_str}]")).strong().color(Color32::from_rgb(0, 220, 255)));
        });

        ui.add_space(12.0);

        if ui.add_sized([ui.available_width(), 36.0], Button::new(RichText::new("💾 Save & Apply Hotkey Changes").strong())).clicked() {
            self.apply_hotkeys();
        }
    }

    fn render_output_tab(&mut self, ui: &mut Ui) {
        ui.heading("📁 Output Destination & File Naming");
        ui.label("Configure where screenshots are stored and how filenames increment.");
        ui.add_space(8.0);

        // Destination Folder Card
        ui.group(|ui| {
            ui.strong("Destination Folder:");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.config.output_dir.to_string_lossy()).monospace());
            });
            ui.horizontal(|ui| {
                if ui.button("📂 Browse Folder...").clicked() {
                    if let Some(folder) = rfd::FileDialog::new()
                        .set_directory(&self.config.output_dir)
                        .pick_folder()
                    {
                        self.config.output_dir = folder;
                        let _ = self.config.save();
                    }
                }
                if ui.button("🧭 Open in Explorer").clicked() {
                    Self::open_folder(&self.config.output_dir);
                }
            });
        });

        ui.add_space(12.0);

        // Format & Quality Card
        ui.group(|ui| {
            ui.strong("Image Format & Encoding:");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for fmt in OutputFormat::ALL {
                    if ui.selectable_value(&mut self.config.format, fmt, fmt.label()).clicked() {
                        let _ = self.config.save();
                    }
                }
            });

            if self.config.format == OutputFormat::Jpeg {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("JPEG Quality:");
                    if ui.add(egui::Slider::new(&mut self.config.jpeg_quality, 1..=100).text("%")).changed() {
                        let _ = self.config.save();
                    }
                });
            }
        });

        ui.add_space(12.0);

        // Sequential Naming & Increment Card
        ui.group(|ui| {
            ui.strong("Incremental File Naming:");
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("Filename Prefix:");
                if ui.text_edit_singleline(&mut self.config.file_prefix).changed() {
                    let _ = self.config.save();
                }

                ui.label("Zero-padding digits:");
                if ui.add(egui::DragValue::new(&mut self.config.padding_digits).range(1..=8)).changed() {
                    let _ = self.config.save();
                }
            });

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Current Number Counter:");
                if ui.add(egui::DragValue::new(&mut self.config.counter).range(1..=999999)).changed() {
                    let _ = self.config.save();
                }

                if ui.button("Reset Counter to 1").clicked() {
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
            ui.label(RichText::new(format!("Next Output Filename: {preview_name}")).strong().color(Color32::from_rgb(0, 220, 255)));

            ui.add_space(6.0);
            ui.separator();
            ui.strong("Session Settings:");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Session Index:");
                if ui.add(egui::DragValue::new(&mut self.config.session_index).range(1..=99999)).changed() {
                    let _ = self.config.save();
                }

                ui.label("Session Prefix:");
                if ui.text_edit_singleline(&mut self.config.session_prefix).changed() {
                    let _ = self.config.save();
                }
            });

            if ui.checkbox(&mut self.config.use_session_subfolders, "Create a dedicated subfolder for each session (e.g. captures/session_01/)").changed() {
                let _ = self.config.save();
            }

            if ui.checkbox(&mut self.config.auto_increment, "Auto-increment number on each capture").changed() {
                let _ = self.config.save();
            }

            if ui.checkbox(&mut self.config.overwrite_existing, "Overwrite file if filename already exists").changed() {
                let _ = self.config.save();
            }

            if ui.checkbox(&mut self.config.play_sound, "Play audio notification on capture").changed() {
                let _ = self.config.save();
            }
        });
    }

    fn render_history_tab(&mut self, ui: &mut Ui) {
        ui.heading("📜 Capture History");
        ui.horizontal(|ui| {
            ui.label(format!("Total captures in current run: {}", self.history.len()));
            if !self.history.is_empty() {
                if ui.button("Clear History").clicked() {
                    self.history.clear();
                }
            }
        });
        ui.add_space(8.0);

        if self.history.is_empty() {
            ui.label(RichText::new("No screenshots captured yet. Use your capture hotkey or the button above!").color(Color32::GRAY));
            return;
        }

        for item in &self.history {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("#{:0width$}", item.counter, width = self.config.padding_digits));
                    ui.label(RichText::new(item.file_path.file_name().unwrap_or_default().to_string_lossy()).strong());
                    ui.label(format!("{}x{} px", item.width, item.height));
                    ui.label(format!("{:.1} KB", item.file_size_bytes as f64 / 1024.0));
                    ui.label(format!("Session #{} ({})", item.session, item.monitor_name));
                    ui.label(RichText::new(&item.timestamp).color(Color32::GRAY));
                });

                ui.horizontal(|ui| {
                    if ui.button("👁️ Open Image").clicked() {
                        Self::open_file(&item.file_path);
                    }
                    if ui.button("📂 Locate in Folder").clicked() {
                        Self::select_in_explorer(&item.file_path);
                    }
                    if ui.button("📋 Copy Image to Clipboard").clicked() {
                        if let Err(e) = Self::copy_image_to_clipboard(&item.file_path) {
                            self.status_message = format!("Clipboard error: {e}");
                        } else {
                            self.status_message = "Image copied to clipboard!".to_string();
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }
    }

    fn render_about_tab(&mut self, ui: &mut Ui) {
        ui.heading("🎯 shotGun");
        ui.label(RichText::new("Fast, Lightweight Screen & ROI Capture Utility").size(15.0).color(Color32::from_rgb(0, 220, 255)));
        ui.add_space(8.0);

        ui.label("shotGun is built in high-performance Rust with native Win32 global hotkeys, immediate-mode GUI (egui), and multi-monitor screen capture (xcap).");
        ui.add_space(8.0);

        ui.group(|ui| {
            ui.strong("Features:");
            ui.label("• Multi-Monitor Detection & Selection");
            ui.label("• Interactive Snipping Overlay (Drag-to-select region)");
            ui.label("• Global Hotkey Engine (Works in any application or game)");
            ui.label("• Configurable Image Formats (PNG, JPEG with quality, BMP, WebP)");
            ui.label("• Sequential Number Increments (e.g. shot_001.png, shot_002.png)");
            ui.label("• 'Start New Session' Hotkey to reset counter or advance session subfolder");
            ui.label("• Audio and visual capture feedback");
            ui.label("• Capture history with instant open, locate, and copy to clipboard");
        });

        ui.add_space(8.0);
        ui.label(RichText::new("Repository: https://github.com/phonie-bytes/shotGun").color(Color32::from_rgb(100, 180, 255)));
    }
}

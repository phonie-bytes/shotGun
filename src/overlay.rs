use crate::annotate::{self, Annotation};
use crate::capture::capture_monitor_raw;
use crate::config::RectRegion;
use egui::{Color32, ColorImage, Context, Key, Pos2, Rect, Stroke, TextureHandle, Vec2};
use image::RgbaImage;

pub enum OverlayAction {
    None,
    Confirmed(RectRegion),
    Cancelled,
    /// A quick-capture annotate session finished: the already-baked,
    /// already-cropped image ready to be saved/copied by the caller.
    /// `copy_only` is true for the toolbar's "Copy" action (clipboard only,
    /// no disk write) and false for "Save" (disk write, plus clipboard too
    /// if the app's own `auto_copy_to_clipboard` setting is on).
    Annotated { image: RgbaImage, region: RectRegion, copy_only: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Arrow,
    Rectangle,
    Highlighter,
    Text,
    Blur,
}

#[derive(PartialEq, Eq)]
enum Mode {
    Selecting,
    Annotating,
}

struct PendingText {
    /// Screen-space position (for placing the inline `TextEdit`).
    screen_pos: Pos2,
    /// Region-local physical-pixel position (for the eventual `Annotation::Text`).
    region_pos: (f32, f32),
    buffer: String,
}

pub struct RegionSelectorOverlay {
    pub is_active: bool,
    start_pos: Option<Pos2>,
    current_pos: Option<Pos2>,
    is_dragging: bool,
    monitor_width: u32,
    monitor_height: u32,
    texture: Option<TextureHandle>,
    /// The frozen monitor screenshot's real pixels, kept alongside the GPU
    /// texture so annotate mode has something to actually crop and bake
    /// into — egui textures aren't readable back on the CPU side.
    raw_image: Option<RgbaImage>,

    /// Whether a confirmed drag on *this* session should flow into annotate
    /// mode rather than immediately returning `Confirmed` — set by the
    /// caller in `start()`, true only for the Quick Region Capture flow.
    annotate_after_select: bool,
    mode: Mode,
    confirmed_region: Option<RectRegion>,
    annotations: Vec<Annotation>,
    current_tool: Tool,
    current_color: Color32,
    pending_text: Option<PendingText>,
}

impl Default for RegionSelectorOverlay {
    fn default() -> Self {
        Self {
            is_active: false,
            start_pos: None,
            current_pos: None,
            is_dragging: false,
            monitor_width: 1920,
            monitor_height: 1080,
            texture: None,
            raw_image: None,
            annotate_after_select: false,
            mode: Mode::Selecting,
            confirmed_region: None,
            annotations: Vec::new(),
            current_tool: Tool::Arrow,
            current_color: Color32::from_rgb(255, 40, 40),
            pending_text: None,
        }
    }
}

impl RegionSelectorOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// `annotate_after_select`: when true (the Quick Region Capture flow),
    /// a confirmed drag enters annotate mode instead of immediately
    /// returning `Confirmed`. The plain "Drag-Select ROI" button flow
    /// passes false and keeps today's immediate-return behavior — that
    /// path exists to define a *reusable* region for repeated fast
    /// captures, and forcing an annotate step onto it would break that.
    pub fn start(&mut self, ctx: &Context, monitor_index: usize, annotate_after_select: bool) -> Result<(), String> {
        let (raw_img, _) = capture_monitor_raw(monitor_index)?;
        let (w, h) = raw_img.dimensions();
        self.monitor_width = w;
        self.monitor_height = h;

        let color_img = ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            raw_img.as_raw(),
        );

        self.texture = Some(ctx.load_texture(
            "screen_freeze",
            color_img,
            egui::TextureOptions::LINEAR,
        ));
        self.raw_image = Some(raw_img);

        self.is_active = true;
        self.start_pos = None;
        self.current_pos = None;
        self.is_dragging = false;
        self.annotate_after_select = annotate_after_select;
        self.mode = Mode::Selecting;
        self.confirmed_region = None;
        self.annotations.clear();
        self.current_tool = Tool::Arrow;
        self.pending_text = None;

        Ok(())
    }

    /// Fully resets overlay state and closes it. Called from every terminal
    /// path (Cancelled, or an annotate session finishing).
    fn close(&mut self) {
        self.is_active = false;
        self.texture = None;
        self.raw_image = None;
        self.mode = Mode::Selecting;
        self.confirmed_region = None;
        self.annotations.clear();
        self.pending_text = None;
    }

    pub fn show(&mut self, ctx: &Context) -> OverlayAction {
        if !self.is_active {
            return OverlayAction::None;
        }

        // Cancel on Escape, from either sub-mode. In annotate mode this
        // discards annotations and the whole capture, matching the
        // selection phase's existing Escape behavior.
        if ctx.input(|i| i.key_pressed(Key::Escape)) && self.pending_text.is_none() {
            self.close();
            return OverlayAction::Cancelled;
        }

        match self.mode {
            Mode::Selecting => self.show_selecting(ctx),
            Mode::Annotating => self.show_annotating(ctx),
        }
    }

    fn show_selecting(&mut self, ctx: &Context) -> OverlayAction {
        let mut result = OverlayAction::None;

        egui::Area::new(egui::Id::new("region_selection_overlay"))
            .fixed_pos(Pos2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let screen_rect = ctx.screen_rect();

                // Allocate sense before using painter
                let response = ui.allocate_rect(screen_rect, egui::Sense::drag());

                if response.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.start_pos = Some(pos);
                        self.current_pos = Some(pos);
                        self.is_dragging = true;
                    }
                }

                if self.is_dragging {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.current_pos = Some(pos);
                    }
                }

                if response.drag_stopped_by(egui::PointerButton::Primary) && self.is_dragging {
                    self.is_dragging = false;
                    if let (Some(start), Some(curr)) = (self.start_pos, self.current_pos) {
                        let sel_rect = Rect::from_two_pos(start, curr);
                        if sel_rect.width() > 5.0 && sel_rect.height() > 5.0 {
                            // Scale coordinates from screen_rect to monitor resolution
                            let scale_x = self.monitor_width as f32 / screen_rect.width();
                            let scale_y = self.monitor_height as f32 / screen_rect.height();

                            let rel_min_x = (sel_rect.min.x - screen_rect.min.x).max(0.0) * scale_x;
                            let rel_min_y = (sel_rect.min.y - screen_rect.min.y).max(0.0) * scale_y;
                            let rel_w = sel_rect.width() * scale_x;
                            let rel_h = sel_rect.height() * scale_y;

                            let region = RectRegion {
                                x: rel_min_x.round() as u32,
                                y: rel_min_y.round() as u32,
                                width: (rel_w.round() as u32).min(self.monitor_width - rel_min_x.round() as u32),
                                height: (rel_h.round() as u32).min(self.monitor_height - rel_min_y.round() as u32),
                            };

                            if self.annotate_after_select {
                                self.confirmed_region = Some(region);
                                self.mode = Mode::Annotating;
                                // Stay active; annotate mode takes over next frame.
                            } else {
                                self.close();
                                result = OverlayAction::Confirmed(region);
                            }
                        }
                    }
                }

                let painter = ui.painter();

                // 1. Draw freeze-frame background
                if let Some(tex) = &self.texture {
                    painter.image(
                        tex.id(),
                        screen_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::from_white_alpha(240),
                    );
                }

                // 2. Dark semi-transparent scrim
                painter.rect_filled(
                    screen_rect,
                    0.0,
                    Color32::from_black_alpha(120),
                );

                // 3. Highlight selection rectangle if dragging or set
                if let (Some(start), Some(curr)) = (self.start_pos, self.current_pos) {
                    let sel_rect = Rect::from_two_pos(start, curr);
                    if sel_rect.width() > 1.0 && sel_rect.height() > 1.0 {
                        // Clear dark overlay for selected region by redrawing image slice
                        if let Some(tex) = &self.texture {
                            let uv_min = Pos2::new(
                                (sel_rect.min.x - screen_rect.min.x) / screen_rect.width(),
                                (sel_rect.min.y - screen_rect.min.y) / screen_rect.height(),
                            );
                            let uv_max = Pos2::new(
                                (sel_rect.max.x - screen_rect.min.x) / screen_rect.width(),
                                (sel_rect.max.y - screen_rect.min.y) / screen_rect.height(),
                            );
                            painter.image(
                                tex.id(),
                                sel_rect,
                                Rect::from_min_max(uv_min, uv_max),
                                Color32::WHITE,
                            );
                        }

                        // Border around selection
                        painter.rect_stroke(
                            sel_rect,
                            0.0,
                            Stroke::new(2.0, Color32::from_rgb(0, 220, 255)),
                        );

                        // Dimension label badge
                        let scale_x = self.monitor_width as f32 / screen_rect.width();
                        let scale_y = self.monitor_height as f32 / screen_rect.height();
                        let actual_w = (sel_rect.width() * scale_x).round() as u32;
                        let actual_h = (sel_rect.height() * scale_y).round() as u32;

                        let text = format!("{} x {} px", actual_w, actual_h);
                        let text_pos = Pos2::new(sel_rect.min.x + 8.0, sel_rect.min.y - 24.0);
                        let badge_pos = if text_pos.y < screen_rect.min.y + 10.0 {
                            Pos2::new(sel_rect.min.x + 8.0, sel_rect.min.y + 8.0)
                        } else {
                            text_pos
                        };

                        painter.rect_filled(
                            Rect::from_min_size(badge_pos, Vec2::new(120.0, 22.0)),
                            4.0,
                            Color32::from_black_alpha(200),
                        );
                        painter.text(
                            Pos2::new(badge_pos.x + 6.0, badge_pos.y + 3.0),
                            egui::Align2::LEFT_TOP,
                            text,
                            egui::FontId::proportional(14.0),
                            Color32::WHITE,
                        );
                    }
                }

                // 3b. Magnifier loupe near the cursor while dragging, for
                // pixel-precise edge alignment: an 8x zoomed sample of the
                // frozen screenshot around the cursor, with a crosshair
                // marking the exact pixel that would be used as the edge.
                if self.is_dragging {
                    if let (Some(cursor), Some(tex)) = (self.current_pos, &self.texture) {
                        const LOUPE_SIZE: f32 = 120.0;
                        const ZOOM: f32 = 8.0;
                        let sample_size_pts = LOUPE_SIZE / ZOOM;

                        // Offset from the cursor so the loupe doesn't cover
                        // the exact point being aligned; flip to whichever
                        // side keeps it fully on-screen near an edge.
                        let mut loupe_pos = cursor + Vec2::new(24.0, 24.0);
                        if loupe_pos.x + LOUPE_SIZE > screen_rect.max.x {
                            loupe_pos.x = cursor.x - 24.0 - LOUPE_SIZE;
                        }
                        if loupe_pos.y + LOUPE_SIZE > screen_rect.max.y {
                            loupe_pos.y = cursor.y - 24.0 - LOUPE_SIZE;
                        }
                        let loupe_rect = Rect::from_min_size(loupe_pos, Vec2::splat(LOUPE_SIZE));

                        let uv_center = Pos2::new(
                            (cursor.x - screen_rect.min.x) / screen_rect.width(),
                            (cursor.y - screen_rect.min.y) / screen_rect.height(),
                        );
                        let half_uv = Vec2::new(
                            (sample_size_pts / 2.0) / screen_rect.width(),
                            (sample_size_pts / 2.0) / screen_rect.height(),
                        );
                        let uv_rect = Rect::from_min_max(uv_center - half_uv, uv_center + half_uv);

                        painter.rect_filled(loupe_rect.expand(2.0), 4.0, Color32::BLACK);
                        painter.image(tex.id(), loupe_rect, uv_rect, Color32::WHITE);
                        painter.rect_stroke(
                            loupe_rect,
                            4.0,
                            Stroke::new(2.0, Color32::from_rgb(0, 220, 255)),
                        );

                        let center = loupe_rect.center();
                        let crosshair = Stroke::new(1.0, Color32::from_rgb(255, 60, 60));
                        painter.line_segment(
                            [Pos2::new(center.x - 8.0, center.y), Pos2::new(center.x + 8.0, center.y)],
                            crosshair,
                        );
                        painter.line_segment(
                            [Pos2::new(center.x, center.y - 8.0), Pos2::new(center.x, center.y + 8.0)],
                            crosshair,
                        );
                    }
                }

                // 4. Instructions banner at the top
                let banner_rect = Rect::from_center_size(
                    Pos2::new(screen_rect.center().x, screen_rect.min.y + 40.0),
                    Vec2::new(520.0, 48.0),
                );
                painter.rect_filled(
                    banner_rect,
                    8.0,
                    Color32::from_black_alpha(220),
                );
                painter.rect_stroke(
                    banner_rect,
                    8.0,
                    Stroke::new(1.0, Color32::from_rgb(70, 130, 250)),
                );
                painter.text(
                    banner_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "🎯 Click and drag to select region | Press Esc to cancel",
                    egui::FontId::proportional(16.0),
                    Color32::WHITE,
                );
            });

        result
    }

    /// Crops the frozen raw image to `region`, bakes `self.annotations`
    /// into it, and closes the overlay. `copy_only` is threaded straight
    /// into the returned `OverlayAction` for the caller to act on.
    fn finish_annotated(&mut self, copy_only: bool) -> OverlayAction {
        let Some(region) = self.confirmed_region else {
            self.close();
            return OverlayAction::Cancelled;
        };
        let Some(raw) = &self.raw_image else {
            self.close();
            return OverlayAction::Cancelled;
        };

        let mut cropped = crate::capture::capture_region(raw, Some(region));
        annotate::bake(&mut cropped, &self.annotations);

        self.close();
        OverlayAction::Annotated { image: cropped, region, copy_only }
    }

    fn show_annotating(&mut self, ctx: &Context) -> OverlayAction {
        let mut result = OverlayAction::None;

        let Some(region) = self.confirmed_region else {
            self.close();
            return OverlayAction::Cancelled;
        };

        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Z)) {
            self.annotations.pop();
        }
        // Ctrl+S / Ctrl+Shift+S mirror the toolbar's Save/Copy buttons, for
        // anyone who'd rather not reach for the mouse mid-annotation.
        if self.pending_text.is_none() && ctx.input(|i| i.key_pressed(Key::S) && i.modifiers.ctrl) {
            let copy_only = ctx.input(|i| i.modifiers.shift);
            return self.finish_annotated(copy_only);
        }
        // Number keys switch tools without reaching for the toolbar —
        // standard UX for annotation/drawing tools. Ignored while typing an
        // in-progress text annotation, so digits go into the text itself.
        if self.pending_text.is_none() {
            ctx.input(|i| {
                if i.key_pressed(Key::Num1) { self.current_tool = Tool::Arrow; }
                if i.key_pressed(Key::Num2) { self.current_tool = Tool::Rectangle; }
                if i.key_pressed(Key::Num3) { self.current_tool = Tool::Highlighter; }
                if i.key_pressed(Key::Num4) { self.current_tool = Tool::Text; }
                if i.key_pressed(Key::Num5) { self.current_tool = Tool::Blur; }
            });
        }

        egui::Area::new(egui::Id::new("region_annotate_overlay"))
            .fixed_pos(Pos2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let screen_rect = ctx.screen_rect();
                // Points per physical pixel, for converting between the
                // screen/point space egui works in and the region-local
                // physical-pixel space annotations are stored in.
                let pts_per_px_x = screen_rect.width() / self.monitor_width as f32;
                let pts_per_px_y = screen_rect.height() / self.monitor_height as f32;

                let region_screen_rect = Rect::from_min_size(
                    Pos2::new(
                        screen_rect.min.x + region.x as f32 * pts_per_px_x,
                        screen_rect.min.y + region.y as f32 * pts_per_px_y,
                    ),
                    Vec2::new(
                        region.width as f32 * pts_per_px_x,
                        region.height as f32 * pts_per_px_y,
                    ),
                );

                let to_region_local = |p: Pos2| -> (f32, f32) {
                    (
                        (p.x - region_screen_rect.min.x) / pts_per_px_x,
                        (p.y - region_screen_rect.min.y) / pts_per_px_y,
                    )
                };
                let to_screen = |local: (f32, f32)| -> Pos2 {
                    Pos2::new(
                        region_screen_rect.min.x + local.0 * pts_per_px_x,
                        region_screen_rect.min.y + local.1 * pts_per_px_y,
                    )
                };

                // Drawing interaction, confined to the ROI. Allocated before
                // taking the painter, since `Ui::painter()` and
                // `Ui::allocate_rect()` can't both hold a borrow of `ui` at
                // once.
                let response = ui.allocate_rect(region_screen_rect, egui::Sense::click_and_drag());

                let painter = ui.painter();

                // Background: dim the full frozen frame, then show the ROI
                // itself at full brightness so it's clear what's in-shot.
                if let Some(tex) = &self.texture {
                    painter.image(
                        tex.id(),
                        screen_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::from_white_alpha(200),
                    );
                    painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(140));

                    let uv_min = Pos2::new(
                        region.x as f32 / self.monitor_width as f32,
                        region.y as f32 / self.monitor_height as f32,
                    );
                    let uv_max = Pos2::new(
                        (region.x + region.width) as f32 / self.monitor_width as f32,
                        (region.y + region.height) as f32 / self.monitor_height as f32,
                    );
                    painter.image(tex.id(), region_screen_rect, Rect::from_min_max(uv_min, uv_max), Color32::WHITE);
                }
                painter.rect_stroke(region_screen_rect, 0.0, Stroke::new(2.0, Color32::from_rgb(0, 220, 255)));

                if self.current_tool == Tool::Text {
                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            self.commit_pending_text();
                            self.pending_text = Some(PendingText {
                                screen_pos: pos,
                                region_pos: to_region_local(pos),
                                buffer: String::new(),
                            });
                        }
                    }
                } else {
                    if response.drag_started() {
                        self.start_pos = response.interact_pointer_pos();
                        self.current_pos = self.start_pos;
                    }
                    if response.dragged() {
                        self.current_pos = response.interact_pointer_pos();
                    }
                    if response.drag_stopped() {
                        if let (Some(s), Some(e)) = (self.start_pos, self.current_pos) {
                            if (s - e).length() > 3.0 {
                                let sl = to_region_local(s);
                                let el = to_region_local(e);
                                let color = self.current_color.to_array();
                                let ann = match self.current_tool {
                                    Tool::Arrow => Some(Annotation::Arrow { start: sl, end: el, color }),
                                    Tool::Rectangle => Some(Annotation::Rectangle { rect: rect_from_local(sl, el), color }),
                                    Tool::Highlighter => Some(Annotation::Highlighter { rect: rect_from_local(sl, el), color }),
                                    Tool::Blur => Some(Annotation::Blur { rect: rect_from_local(sl, el) }),
                                    Tool::Text => None,
                                };
                                if let Some(a) = ann {
                                    self.annotations.push(a);
                                }
                            }
                        }
                        self.start_pos = None;
                        self.current_pos = None;
                    }
                }

                // Live preview of already-committed annotations.
                for ann in &self.annotations {
                    draw_annotation_preview(painter, ann, to_screen);
                }

                // Live preview of the in-progress drag.
                if let (Some(s), Some(e)) = (self.start_pos, self.current_pos) {
                    match self.current_tool {
                        Tool::Arrow => {
                            painter.line_segment([s, e], Stroke::new(3.0, self.current_color));
                        }
                        Tool::Rectangle => {
                            painter.rect_stroke(Rect::from_two_pos(s, e), 0.0, Stroke::new(2.0, self.current_color));
                        }
                        Tool::Highlighter => {
                            painter.rect_filled(Rect::from_two_pos(s, e), 0.0, self.current_color.gamma_multiply(0.35));
                        }
                        Tool::Blur => {
                            painter.rect_filled(Rect::from_two_pos(s, e), 0.0, Color32::from_gray(128).gamma_multiply(0.5));
                        }
                        Tool::Text => {}
                    }
                }

                // Inline text input, shown at the click point until Enter/Esc.
                let mut commit_text = false;
                let mut cancel_text = false;
                if let Some(pending) = &mut self.pending_text {
                    egui::Area::new(egui::Id::new("annotate_text_input"))
                        .fixed_pos(pending.screen_pos)
                        .order(egui::Order::Tooltip)
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut pending.buffer)
                                        .hint_text("Type, Enter to place")
                                        .desired_width(180.0),
                                );
                                resp.request_focus();
                            });
                        });
                    if ctx.input(|i| i.key_pressed(Key::Enter)) {
                        commit_text = true;
                    } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
                        cancel_text = true;
                    }
                }
                if commit_text {
                    self.commit_pending_text();
                }
                if cancel_text {
                    self.pending_text = None;
                }

                // Toolbar.
                egui::Area::new(egui::Id::new("annotate_toolbar"))
                    .fixed_pos(Pos2::new(screen_rect.min.x + 12.0, screen_rect.min.y + 12.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.current_tool, Tool::Arrow, "➡ Arrow").on_hover_text("1");
                                ui.selectable_value(&mut self.current_tool, Tool::Rectangle, "▭ Rect").on_hover_text("2");
                                ui.selectable_value(&mut self.current_tool, Tool::Highlighter, "🖍 Highlight").on_hover_text("3");
                                ui.selectable_value(&mut self.current_tool, Tool::Text, "🔤 Text").on_hover_text("4");
                                ui.selectable_value(&mut self.current_tool, Tool::Blur, "🌫 Blur").on_hover_text("5");
                                ui.separator();
                                if self.current_tool != Tool::Blur {
                                    ui.color_edit_button_srgba(&mut self.current_color);
                                }
                                ui.separator();
                                if ui.button("↩ Undo").on_hover_text("Ctrl+Z").clicked() {
                                    self.annotations.pop();
                                }
                                if ui.button("🗑 Clear").clicked() {
                                    self.annotations.clear();
                                }
                                ui.separator();
                                if ui.button(egui::RichText::new("💾 Save").strong()).clicked() {
                                    result = self.finish_annotated(false);
                                }
                                if ui.button("📋 Copy").clicked() {
                                    result = self.finish_annotated(true);
                                }
                                if ui.button("❌ Cancel").clicked() {
                                    self.close();
                                    result = OverlayAction::Cancelled;
                                }
                            });
                        });
                    });
            });

        result
    }

    fn commit_pending_text(&mut self) {
        if let Some(pending) = self.pending_text.take() {
            if !pending.buffer.trim().is_empty() {
                self.annotations.push(Annotation::Text {
                    pos: pending.region_pos,
                    text: pending.buffer,
                    color: self.current_color.to_array(),
                    size: 20.0,
                });
            }
        }
    }
}

fn rect_from_local(a: (f32, f32), b: (f32, f32)) -> (f32, f32, f32, f32) {
    let x = a.0.min(b.0);
    let y = a.1.min(b.1);
    let w = (a.0 - b.0).abs();
    let h = (a.1 - b.1).abs();
    (x, y, w, h)
}

/// Renders a rough live preview of a committed annotation in screen space.
/// Doesn't need to pixel-match `annotate::bake`'s final raster exactly —
/// it's only shown while the user is still working, before the real bake.
fn draw_annotation_preview(painter: &egui::Painter, ann: &Annotation, to_screen: impl Fn((f32, f32)) -> Pos2) {
    match ann {
        Annotation::Arrow { start, end, color } => {
            let s = to_screen(*start);
            let e = to_screen(*end);
            painter.line_segment([s, e], Stroke::new(3.0, color32_from(*color)));
            painter.circle_filled(e, 4.0, color32_from(*color));
        }
        Annotation::Rectangle { rect, color } => {
            let r = local_rect_to_screen(*rect, &to_screen);
            painter.rect_stroke(r, 0.0, Stroke::new(2.0, color32_from(*color)));
        }
        Annotation::Highlighter { rect, color } => {
            let r = local_rect_to_screen(*rect, &to_screen);
            painter.rect_filled(r, 0.0, color32_from(*color).gamma_multiply(0.35));
        }
        Annotation::Blur { rect } => {
            let r = local_rect_to_screen(*rect, &to_screen);
            painter.rect_filled(r, 0.0, Color32::from_gray(128).gamma_multiply(0.5));
        }
        Annotation::Text { pos, text, color, size } => {
            let p = to_screen(*pos);
            painter.text(p, egui::Align2::LEFT_TOP, text, egui::FontId::proportional(*size), color32_from(*color));
        }
    }
}

/// Maps a region-local `(x, y, w, h)` rect to a screen-space `Rect` via
/// `to_screen`, deriving the scale by probing it at the origin and at
/// `(1,1)` — avoids threading `pts_per_px` through as a second parameter
/// just for size conversion.
fn local_rect_to_screen(rect: (f32, f32, f32, f32), to_screen: &impl Fn((f32, f32)) -> Pos2) -> Rect {
    let (x, y, w, h) = rect;
    let origin = to_screen((0.0, 0.0));
    let unit = to_screen((1.0, 1.0));
    let scale_x = unit.x - origin.x;
    let scale_y = unit.y - origin.y;
    Rect::from_min_size(to_screen((x, y)), Vec2::new(w * scale_x, h * scale_y))
}

fn color32_from(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

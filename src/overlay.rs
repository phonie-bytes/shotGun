use crate::capture::capture_monitor_raw;
use crate::config::RectRegion;
use egui::{Color32, ColorImage, Context, Key, Pos2, Rect, Stroke, TextureHandle, Vec2};

pub enum OverlayAction {
    None,
    Confirmed(RectRegion),
    Cancelled,
}

pub struct RegionSelectorOverlay {
    pub is_active: bool,
    start_pos: Option<Pos2>,
    current_pos: Option<Pos2>,
    is_dragging: bool,
    monitor_width: u32,
    monitor_height: u32,
    texture: Option<TextureHandle>,
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
        }
    }
}

impl RegionSelectorOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, ctx: &Context, monitor_index: usize) -> Result<(), String> {
        let (raw_img, _) = capture_monitor_raw(monitor_index)?;
        let (w, h) = raw_img.dimensions();
        self.monitor_width = w;
        self.monitor_height = h;

        // Convert image to egui ColorImage
        let color_img = ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            raw_img.as_raw(),
        );

        self.texture = Some(ctx.load_texture(
            "screen_freeze",
            color_img,
            egui::TextureOptions::LINEAR,
        ));

        self.is_active = true;
        self.start_pos = None;
        self.current_pos = None;
        self.is_dragging = false;

        Ok(())
    }

    pub fn show(&mut self, ctx: &Context) -> OverlayAction {
        if !self.is_active {
            return OverlayAction::None;
        }

        // Cancel on Escape
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.is_active = false;
            self.texture = None;
            return OverlayAction::Cancelled;
        }

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

                            self.is_active = false;
                            self.texture = None;
                            result = OverlayAction::Confirmed(region);
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
}

//! Post-capture annotation: a small ordered list of shapes drawn in
//! region-local *physical pixel* space (relative to the cropped ROI image's
//! own top-left, independent of monitor/DPI/screen coordinates), and a
//! hand-rolled rasterizer (`bake`) that burns them into the final
//! `RgbaImage` before it's saved or copied. No new dependencies: text uses a
//! small hand-authored bitmap font instead of pulling in a font-rendering
//! crate or bundling a licensed font file.

use image::{Rgba, RgbaImage};

#[derive(Debug, Clone)]
pub enum Annotation {
    Arrow { start: (f32, f32), end: (f32, f32), color: [u8; 4] },
    Rectangle { rect: (f32, f32, f32, f32), color: [u8; 4] },
    Highlighter { rect: (f32, f32, f32, f32), color: [u8; 4] },
    Text { pos: (f32, f32), text: String, color: [u8; 4], size: f32 },
    /// `strength` is 0.0-1.0, controlling the pixelation block size (see
    /// `draw_pixelate`) — higher means bigger blocks, i.e. more thoroughly
    /// destroyed detail.
    Blur { rect: (f32, f32, f32, f32), strength: f32 },
    /// A fully opaque solid-fill box — for when blur/pixelation isn't
    /// foolproof enough and the content underneath should be completely
    /// gone, not just illegible (e.g. redacting a password field outright).
    Redact { rect: (f32, f32, f32, f32), color: [u8; 4] },
    /// A numbered badge (for tutorial-style "step 1, step 2, ..." callouts).
    /// `number` is fixed at the moment it's placed, not recomputed at bake
    /// time — undoing a step label is what's responsible for handing the
    /// next click back the same number (see `RegionSelectorOverlay::undo`).
    StepLabel { pos: (f32, f32), number: u32, color: [u8; 4] },
}

/// Applies every annotation, in order, onto `image`. Later entries draw on
/// top of earlier ones — the same order an undo stack should pop from.
pub fn bake(image: &mut RgbaImage, annotations: &[Annotation]) {
    for a in annotations {
        match a {
            Annotation::Arrow { start, end, color } => draw_arrow(image, *start, *end, *color),
            Annotation::Rectangle { rect, color } => draw_rect_border(image, *rect, *color),
            Annotation::Highlighter { rect, color } => draw_highlight(image, *rect, *color),
            Annotation::Text { pos, text, color, size } => draw_text(image, *pos, text, *color, *size),
            Annotation::Blur { rect, strength } => draw_pixelate(image, *rect, *strength),
            Annotation::Redact { rect, color } => draw_redact(image, *rect, *color),
            Annotation::StepLabel { pos, number, color } => draw_step_label(image, *pos, *number, *color),
        }
    }
}

/// Standard "over" alpha compositing of `color` onto the pixel at `(x, y)`,
/// bounds-checked. The output is always fully opaque (alpha 255) — this is
/// baking into a final screenshot raster, not compositing further layers.
fn blend_pixel(image: &mut RgbaImage, x: i32, y: i32, color: [u8; 4]) {
    let (w, h) = image.dimensions();
    if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
        return;
    }
    let alpha = color[3] as f32 / 255.0;
    if alpha <= 0.0 {
        return;
    }
    let px = image.get_pixel_mut(x as u32, y as u32);
    if alpha >= 1.0 {
        px.0 = [color[0], color[1], color[2], 255];
        return;
    }
    for i in 0..3 {
        let blended = color[i] as f32 * alpha + px.0[i] as f32 * (1.0 - alpha);
        px.0[i] = blended.round().clamp(0.0, 255.0) as u8;
    }
    px.0[3] = 255;
}

fn normalize_rect(rect: (f32, f32, f32, f32)) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = rect;
    let x0 = x.min(x + w) as i32;
    let y0 = y.min(y + h) as i32;
    let x1 = (x + w).max(x) as i32;
    let y1 = (y + h).max(y) as i32;
    (x0, y0, x1, y1)
}

fn draw_rect_border(image: &mut RgbaImage, rect: (f32, f32, f32, f32), color: [u8; 4]) {
    const THICKNESS: i32 = 3;
    let (x0, y0, x1, y1) = normalize_rect(rect);
    for yy in y0..y1 {
        for xx in x0..x1 {
            let near_edge = xx - x0 < THICKNESS
                || x1 - 1 - xx < THICKNESS
                || yy - y0 < THICKNESS
                || y1 - 1 - yy < THICKNESS;
            if near_edge {
                blend_pixel(image, xx, yy, color);
            }
        }
    }
}

/// Filled rect at a fixed, low alpha regardless of the picked color's own
/// alpha — a highlighter should always look like a highlighter, without
/// the user needing to separately dial down opacity for this one tool.
fn draw_highlight(image: &mut RgbaImage, rect: (f32, f32, f32, f32), color: [u8; 4]) {
    let (x0, y0, x1, y1) = normalize_rect(rect);
    let c = [color[0], color[1], color[2], 90];
    for yy in y0..y1 {
        for xx in x0..x1 {
            blend_pixel(image, xx, yy, c);
        }
    }
}

/// Pixelate (mosaic) the rect: each block is flattened to its own average
/// color. More foolproof than a gaussian blur for actually destroying the
/// readability of redacted text. `strength` (0.0-1.0) controls the block
/// size — from a light 6px mosaic up to a 40px block that erases almost
/// all detail — rather than a single fixed size for every use.
fn draw_pixelate(image: &mut RgbaImage, rect: (f32, f32, f32, f32), strength: f32) {
    let block = (6.0 + strength.clamp(0.0, 1.0) * 34.0).round() as u32;
    let (w_img, h_img) = image.dimensions();
    let (x0, y0, x1, y1) = normalize_rect(rect);
    let x0 = x0.max(0) as u32;
    let y0 = y0.max(0) as u32;
    let x1 = (x1.max(0) as u32).min(w_img);
    let y1 = (y1.max(0) as u32).min(h_img);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let mut by = y0;
    while by < y1 {
        let bh = block.min(y1 - by);
        let mut bx = x0;
        while bx < x1 {
            let bw = block.min(x1 - bx);

            let mut sum = [0u64; 3];
            let mut count = 0u64;
            for yy in by..by + bh {
                for xx in bx..bx + bw {
                    let p = image.get_pixel(xx, yy);
                    sum[0] += p.0[0] as u64;
                    sum[1] += p.0[1] as u64;
                    sum[2] += p.0[2] as u64;
                    count += 1;
                }
            }
            if count > 0 {
                let avg = Rgba([
                    (sum[0] / count) as u8,
                    (sum[1] / count) as u8,
                    (sum[2] / count) as u8,
                    255,
                ]);
                for yy in by..by + bh {
                    for xx in bx..bx + bw {
                        image.put_pixel(xx, yy, avg);
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
}

/// A fully opaque solid fill — unlike `draw_highlight` (translucent) or
/// `draw_pixelate` (mosaic, technically still derived from the original
/// pixels), this completely replaces the rect's content with a flat color.
/// For redacting something that must be gone entirely, not just illegible.
fn draw_redact(image: &mut RgbaImage, rect: (f32, f32, f32, f32), color: [u8; 4]) {
    let (x0, y0, x1, y1) = normalize_rect(rect);
    let opaque = [color[0], color[1], color[2], 255];
    for yy in y0..y1 {
        for xx in x0..x1 {
            blend_pixel(image, xx, yy, opaque);
        }
    }
}

fn point_segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py) = p;
    let (ax, ay) = a;
    let (bx, by) = b;
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0001 {
        (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn draw_thick_line(image: &mut RgbaImage, start: (f32, f32), end: (f32, f32), thickness: f32, color: [u8; 4]) {
    let (w_img, h_img) = image.dimensions();
    let x0 = (start.0.min(end.0) - thickness).floor().max(0.0) as i32;
    let x1 = (start.0.max(end.0) + thickness).ceil().min(w_img as f32) as i32;
    let y0 = (start.1.min(end.1) - thickness).floor().max(0.0) as i32;
    let y1 = (start.1.max(end.1) + thickness).ceil().min(h_img as f32) as i32;
    let half = thickness / 2.0;

    for yy in y0..y1 {
        for xx in x0..x1 {
            let d = point_segment_distance((xx as f32 + 0.5, yy as f32 + 0.5), start, end);
            if d <= half {
                blend_pixel(image, xx, yy, color);
            }
        }
    }
}

fn sign(p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) -> f32 {
    (p1.0 - p3.0) * (p2.1 - p3.1) - (p2.0 - p3.0) * (p1.1 - p3.1)
}

fn point_in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let d1 = sign(p, a, b);
    let d2 = sign(p, b, c);
    let d3 = sign(p, c, a);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

fn fill_triangle(image: &mut RgbaImage, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: [u8; 4]) {
    let (w_img, h_img) = image.dimensions();
    let min_x = a.0.min(b.0).min(c.0).floor().max(0.0) as i32;
    let max_x = a.0.max(b.0).max(c.0).ceil().min(w_img as f32) as i32;
    let min_y = a.1.min(b.1).min(c.1).floor().max(0.0) as i32;
    let max_y = a.1.max(b.1).max(c.1).ceil().min(h_img as f32) as i32;

    for yy in min_y..max_y {
        for xx in min_x..max_x {
            let p = (xx as f32 + 0.5, yy as f32 + 0.5);
            if point_in_triangle(p, a, b, c) {
                blend_pixel(image, xx, yy, color);
            }
        }
    }
}

fn draw_arrow(image: &mut RgbaImage, start: (f32, f32), end: (f32, f32), color: [u8; 4]) {
    const THICKNESS: f32 = 4.0;
    draw_thick_line(image, start, end, THICKNESS, color);

    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let ux = dx / len;
    let uy = dy / len;

    let head_len = 14.0_f32.min(len);
    let head_width = 9.0_f32;
    let back = (end.0 - ux * head_len, end.1 - uy * head_len);
    // Perpendicular to the line direction, for the arrowhead's two back corners.
    let perp = (-uy, ux);
    let p1 = (back.0 + perp.0 * head_width, back.1 + perp.1 * head_width);
    let p2 = (back.0 - perp.0 * head_width, back.1 - perp.1 * head_width);

    fill_triangle(image, end, p1, p2, color);
}

fn draw_text(image: &mut RgbaImage, pos: (f32, f32), text: &str, color: [u8; 4], size: f32) {
    let scale = (size / 8.0).max(1.0);
    let mut cursor_x = pos.0;
    let cursor_y = pos.1;
    for ch in text.chars() {
        draw_glyph(image, cursor_x, cursor_y, ch, color, scale);
        cursor_x += (6.0 * scale) + scale; // 5px glyph + 1px gap, both scaled
    }
}

/// A filled circular badge with a centered white number — "step 1, step 2,
/// ..." callouts for tutorial-style screenshots. `color` is whatever the
/// annotate toolbar's color picker was set to when this was placed, so
/// different badges can be different colors just by changing the picker
/// between placements.
fn draw_step_label(image: &mut RgbaImage, pos: (f32, f32), number: u32, color: [u8; 4]) {
    const RADIUS: f32 = 13.0;
    let (cx, cy) = pos;

    let min_x = (cx - RADIUS).floor() as i32;
    let max_x = (cx + RADIUS).ceil() as i32;
    let min_y = (cy - RADIUS).floor() as i32;
    let max_y = (cy + RADIUS).ceil() as i32;
    for yy in min_y..=max_y {
        for xx in min_x..=max_x {
            let dx = xx as f32 + 0.5 - cx;
            let dy = yy as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= RADIUS * RADIUS {
                blend_pixel(image, xx, yy, color);
            }
        }
    }

    let text = number.to_string();
    const SIZE: f32 = 16.0;
    let scale = (SIZE / 8.0).max(1.0);
    let char_advance = 7.0 * scale; // matches draw_text's own per-char step
    let text_w = text.chars().count() as f32 * char_advance - scale;
    let text_h = 7.0 * scale;
    let text_pos = (cx - text_w / 2.0, cy - text_h / 2.0);
    draw_text(image, text_pos, &text, [255, 255, 255, 255], SIZE);
}

fn draw_glyph(image: &mut RgbaImage, x: f32, y: f32, ch: char, color: [u8; 4], scale: f32) {
    let glyph = font_glyph(ch);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            // Bit 4 is the leftmost column.
            if (bits >> (4 - col)) & 1 == 1 {
                let px0 = x + col as f32 * scale;
                let py0 = y + row as f32 * scale;
                let block = scale.ceil().max(1.0) as i32;
                let bx0 = px0.floor() as i32;
                let by0 = py0.floor() as i32;
                for dy in 0..block {
                    for dx in 0..block {
                        blend_pixel(image, bx0 + dx, by0 + dy, color);
                    }
                }
            }
        }
    }
}

/// A compact 5x7 hand-drawn bitmap font. Each glyph is 7 rows, each row's
/// low 5 bits giving the pixel pattern (bit 4 = leftmost column).
///
/// Only one case is drawn — lowercase letters render as their uppercase
/// glyph — trading true lower/upper distinction for a much smaller,
/// entirely self-authored table. At the small sizes this is used for, the
/// difference isn't legibility-critical. Deliberately hand-authored rather
/// than sourced from an existing font file, both to avoid a licensing
/// question and to keep this dependency-free.
fn font_glyph(ch: char) -> [u8; 7] {
    let c = ch.to_ascii_uppercase();
    match c {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01111, 0b10000, 0b10000, 0b10011, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10101, 0b10011, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100, 0b01000],
        ':' => [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000],
        ';' => [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b01000],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100],
        '?' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00100, 0b00000, 0b00100],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '_' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        '@' => [0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01111],
        '#' => [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        '[' => [0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110],
        ']' => [0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110],
        '\'' => [0b01000, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        '"' => [0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111], // box for anything unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_pixel_full_alpha_replaces_color() {
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
        blend_pixel(&mut img, 1, 1, [255, 0, 0, 255]);
        assert_eq!(*img.get_pixel(1, 1), Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn blend_pixel_out_of_bounds_is_noop() {
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
        blend_pixel(&mut img, 100, 100, [255, 0, 0, 255]);
        blend_pixel(&mut img, -1, 0, [255, 0, 0, 255]);
        // Should not panic, and nothing in-bounds should change.
        assert_eq!(*img.get_pixel(0, 0), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn draw_pixelate_flattens_block_to_average() {
        let mut img = RgbaImage::from_pixel(24, 24, Rgba([0, 0, 0, 255]));
        img.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        bake(&mut img, &[Annotation::Blur { rect: (0.0, 0.0, 12.0, 12.0), strength: 0.5 }]);
        // The single white pixel should have been averaged away within its block.
        let p = img.get_pixel(0, 0);
        assert!(p.0[0] < 255, "expected the block average to dilute the single bright pixel");
    }

    /// Builds a 40x40 image, dark on the left half and bright on the right.
    fn half_bright_image() -> RgbaImage {
        let mut img = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 255]));
        for y in 0..40 {
            for x in 20..40 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }

    #[test]
    fn draw_pixelate_strength_one_flattens_whole_rect_to_one_color() {
        let mut img = half_bright_image();
        bake(&mut img, &[Annotation::Blur { rect: (0.0, 0.0, 40.0, 40.0), strength: 1.0 }]);
        // At strength 1.0 the block is as big as the whole rect, so it
        // should all become a single averaged color.
        let corner = *img.get_pixel(0, 0);
        let far_corner = *img.get_pixel(39, 39);
        assert_eq!(corner, far_corner, "a block as big as the whole rect should flatten it to one uniform color");
        assert!(corner.0[0] > 0 && corner.0[0] < 255, "should be an average of the bright and dark halves, not either extreme");
    }

    #[test]
    fn draw_pixelate_low_strength_keeps_some_block_variation() {
        let mut img = half_bright_image();
        bake(&mut img, &[Annotation::Blur { rect: (0.0, 0.0, 40.0, 40.0), strength: 0.0 }]);
        // At strength 0.0 the block is small (6px), so it shouldn't reach
        // across the whole rect — the dark and bright halves should still
        // be distinguishable from each other.
        let dark_side = img.get_pixel(2, 2).0[0];
        let bright_side = img.get_pixel(38, 2).0[0];
        assert!(bright_side > dark_side, "small blocks should still preserve the difference between the bright and dark halves");
    }

    #[test]
    fn draw_redact_fully_replaces_not_blends() {
        let mut img = RgbaImage::from_pixel(10, 10, Rgba([200, 200, 200, 255]));
        bake(&mut img, &[Annotation::Redact { rect: (0.0, 0.0, 5.0, 5.0), color: [10, 10, 10, 255] }]);
        assert_eq!(*img.get_pixel(2, 2), Rgba([10, 10, 10, 255]), "redaction should fully replace the pixel, not blend with the background");
        assert_eq!(*img.get_pixel(7, 7), Rgba([200, 200, 200, 255]), "outside the rect should be untouched");
    }

    #[test]
    fn draw_rect_border_only_touches_border_band() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        bake(&mut img, &[Annotation::Rectangle { rect: (2.0, 2.0, 10.0, 10.0), color: [255, 0, 0, 255] }]);
        assert_eq!(*img.get_pixel(2, 2), Rgba([255, 0, 0, 255]));
        assert_eq!(*img.get_pixel(6, 6), Rgba([0, 0, 0, 255]), "interior of the rectangle should be untouched");
    }

    #[test]
    fn draw_highlight_blends_not_replaces() {
        let mut img = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        bake(&mut img, &[Annotation::Highlighter { rect: (0.0, 0.0, 5.0, 5.0), color: [255, 255, 0, 255] }]);
        let p = img.get_pixel(2, 2);
        assert!(p.0[0] > 0 && p.0[0] < 255, "highlight should partially blend, not fully replace");
    }

    #[test]
    fn draw_text_lights_up_some_pixels() {
        let mut img = RgbaImage::from_pixel(40, 20, Rgba([0, 0, 0, 255]));
        bake(&mut img, &[Annotation::Text { pos: (2.0, 2.0), text: "A".to_string(), color: [255, 255, 255, 255], size: 8.0 }]);
        let lit = img.pixels().filter(|p| p.0[0] > 0).count();
        assert!(lit > 0, "expected the glyph to light up at least one pixel");
    }

    #[test]
    fn draw_step_label_paints_badge_and_number() {
        let mut img = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 255]));
        bake(&mut img, &[Annotation::StepLabel { pos: (20.0, 20.0), number: 1, color: [255, 40, 40, 255] }]);
        // Near the edge of the badge circle: should be the fill color, not background.
        let edge = img.get_pixel(20, 8);
        assert_eq!(*edge, Rgba([255, 40, 40, 255]), "expected the badge circle to be filled with its color");
        // Somewhere in the middle should have picked up white from the number glyph.
        let center_ish = img.get_pixel(19, 20);
        assert!(center_ish.0[0] == 255 && center_ish.0[1] == 255, "expected the centered number to be drawn in white");
    }

    #[test]
    fn draw_step_label_two_digit_number_stays_roughly_centered() {
        // Mostly a smoke test that multi-digit numbers don't panic or run
        // off the badge — exact pixel placement isn't asserted.
        let mut img = RgbaImage::from_pixel(60, 60, Rgba([0, 0, 0, 255]));
        bake(&mut img, &[Annotation::StepLabel { pos: (30.0, 30.0), number: 12, color: [0, 120, 255, 255] }]);
        let lit = img.pixels().filter(|p| p.0[0] > 0 || p.0[1] > 0 || p.0[2] > 0).count();
        assert!(lit > 0);
    }
}

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
    Blur { rect: (f32, f32, f32, f32) },
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
            Annotation::Blur { rect } => draw_pixelate(image, *rect),
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

/// Pixelate (mosaic) the rect: each `BLOCK`x`BLOCK` cell is flattened to its
/// own average color. More foolproof than a gaussian blur for actually
/// destroying the readability of redacted text.
fn draw_pixelate(image: &mut RgbaImage, rect: (f32, f32, f32, f32)) {
    const BLOCK: u32 = 12;
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
        let bh = BLOCK.min(y1 - by);
        let mut bx = x0;
        while bx < x1 {
            let bw = BLOCK.min(x1 - bx);

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
            bx += BLOCK;
        }
        by += BLOCK;
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
        bake(&mut img, &[Annotation::Blur { rect: (0.0, 0.0, 12.0, 12.0) }]);
        // The single white pixel should have been averaged away within its block.
        let p = img.get_pixel(0, 0);
        assert!(p.0[0] < 255, "expected the block average to dilute the single bright pixel");
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
}

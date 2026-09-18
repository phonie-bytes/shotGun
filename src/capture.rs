use crate::config::{AppConfig, OutputFormat, RectRegion};
use chrono::Local;
use image::codecs::bmp::BmpEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ColorType, DynamicImage, ImageEncoder, RgbaImage};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use xcap::Monitor;

#[link(name = "user32")]
extern "system" {
    fn MessageBeep(uType: u32) -> i32;
}

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    #[allow(dead_code)]
    pub index: usize,
    pub name: String,

    #[allow(dead_code)]
    pub x: i32,
    #[allow(dead_code)]
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub file_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub file_size_bytes: u64,
    pub timestamp: String,
    #[allow(dead_code)]
    pub monitor_name: String,
    pub counter: u64,
    #[allow(dead_code)]
    pub session: u64,
}


pub fn get_monitors() -> Vec<MonitorInfo> {
    match Monitor::all() {
        Ok(monitors) => monitors
            .into_iter()
            .enumerate()
            .map(|(idx, m)| MonitorInfo {
                index: idx,
                name: {
                    let n = m.name();
                    if n.is_empty() {
                        format!("Display {}", idx + 1)
                    } else {
                        n.to_string()
                    }
                },
                x: m.x(),
                y: m.y(),
                width: m.width(),
                height: m.height(),
                scale_factor: m.scale_factor(),
                is_primary: m.is_primary(),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn capture_monitor_raw(monitor_index: usize) -> Result<(RgbaImage, String), String> {
    let monitors = Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    let monitor = monitors
        .get(monitor_index)
        .or_else(|| monitors.first())
        .ok_or_else(|| "No monitors found".to_string())?;

    let name = {
        let n = monitor.name();
        if n.is_empty() {
            format!("Monitor {}", monitor_index + 1)
        } else {
            n.to_string()
        }
    };
    let img = monitor
        .capture_image()
        .map_err(|e| format!("Failed to capture monitor: {e}"))?;

    Ok((img, name))
}

pub fn capture_region(
    img: &RgbaImage,
    region: Option<RectRegion>,
) -> RgbaImage {
    let (img_w, img_h) = img.dimensions();
    if let Some(r) = region {
        if r.width == 0 || r.height == 0 {
            return img.clone();
        }
        let x = r.x.min(img_w.saturating_sub(1));
        let y = r.y.min(img_h.saturating_sub(1));
        let w = r.width.max(1).min(img_w - x);
        let h = r.height.max(1).min(img_h - y);

        image::imageops::crop_imm(img, x, y, w, h).to_image()
    } else {
        img.clone()
    }
}

pub fn save_image_with_format(
    img: &RgbaImage,
    format: OutputFormat,
    jpeg_quality: u8,
    dest_path: &Path,
) -> Result<(), String> {
    let file = File::create(dest_path).map_err(|e| format!("Failed to create file {}: {e}", dest_path.display()))?;
    let mut writer = BufWriter::new(file);
    let (w, h) = img.dimensions();

    match format {
        OutputFormat::Png => {
            let encoder = PngEncoder::new(&mut writer);
            encoder
                .write_image(img.as_raw(), w, h, ColorType::Rgba8.into())
                .map_err(|e| format!("PNG encode error: {e}"))?;
        }
        OutputFormat::Jpeg => {
            let rgb_img = DynamicImage::ImageRgba8(img.clone()).into_rgb8();
            let mut encoder = JpegEncoder::new_with_quality(&mut writer, jpeg_quality.clamp(1, 100));
            encoder
                .encode(rgb_img.as_raw(), w, h, ColorType::Rgb8.into())
                .map_err(|e| format!("JPEG encode error: {e}"))?;
        }
        OutputFormat::Bmp => {
            let mut encoder = BmpEncoder::new(&mut writer);
            encoder
                .encode(img.as_raw(), w, h, ColorType::Rgba8.into())
                .map_err(|e| format!("BMP encode error: {e}"))?;
        }
        OutputFormat::WebP => {
            let encoder = WebPEncoder::new_lossless(&mut writer);
            encoder
                .write_image(img.as_raw(), w, h, ColorType::Rgba8.into())
                .map_err(|e| format!("WebP encode error: {e}"))?;
        }
    }

    Ok(())
}

pub fn execute_capture(config: &mut AppConfig) -> Result<CaptureResult, String> {
    // 1. Capture screen
    let (raw_img, monitor_name) = capture_monitor_raw(config.monitor_index)?;

    // 2. Crop to selected region
    let final_img = capture_region(&raw_img, config.region);

    save_captured_image(config, &final_img, monitor_name)
}

/// Writes an already-captured (and possibly already-annotated) image to
/// disk using the session's naming/counter/subfolder logic, and updates
/// `config`'s counter — the same bookkeeping `execute_capture` does, minus
/// the actual screen capture step. Used directly by the post-capture
/// annotate flow, which already has real pixels in hand (baked from the
/// overlay's own frozen frame) and shouldn't re-capture the screen.
pub fn save_captured_image(
    config: &mut AppConfig,
    final_img: &RgbaImage,
    monitor_name: String,
) -> Result<CaptureResult, String> {
    let (width, height) = final_img.dimensions();

    // 3. Prepare target directory
    let target_dir = if config.use_session_subfolders {
        config
            .output_dir
            .join(format!("{}{:02}", config.session_prefix, config.session_index))
    } else {
        config.output_dir.clone()
    };

    std::fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Failed to create destination directory: {e}"))?;

    // 4. Determine filename with increment
    let mut current_counter = config.counter;
    let ext = config.format.extension();
    let file_path = loop {
        let filename = format!(
            "{}{:0width$}.{}",
            config.file_prefix,
            current_counter,
            ext,
            width = config.padding_digits
        );
        let path = target_dir.join(&filename);

        if config.overwrite_existing || !path.exists() {
            break path;
        }

        current_counter += 1;
    };

    // 5. Save the image
    save_image_with_format(final_img, config.format, config.jpeg_quality, &file_path)?;

    let file_size_bytes = std::fs::metadata(&file_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let result = CaptureResult {
        file_path: file_path.clone(),
        width,
        height,
        file_size_bytes,
        timestamp,
        monitor_name,
        counter: current_counter,
        session: config.session_index,
    };

    // 6. Update counter in config
    if config.auto_increment {
        config.counter = current_counter + 1;
        let _ = config.save();
    } else if current_counter != config.counter {
        config.counter = current_counter;
        let _ = config.save();
    }

    // 7. Optional audio feedback
    if config.play_sound {
        unsafe {
            MessageBeep(0); // Standard Windows sound
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn test_crop_region_valid() {
        let mut img = RgbaImage::new(100, 100);
        img.put_pixel(10, 10, Rgba([255, 0, 0, 255]));

        let region = RectRegion {
            x: 5,
            y: 5,
            width: 20,
            height: 20,
        };

        let cropped = capture_region(&img, Some(region));
        assert_eq!(cropped.width(), 20);
        assert_eq!(cropped.height(), 20);
        assert_eq!(*cropped.get_pixel(5, 5), Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn test_crop_region_out_of_bounds() {
        let img = RgbaImage::new(50, 50);
        let region = RectRegion {
            x: 40,
            y: 40,
            width: 100,
            height: 100,
        };

        let cropped = capture_region(&img, Some(region));
        assert_eq!(cropped.width(), 10);
        assert_eq!(cropped.height(), 10);
    }
}


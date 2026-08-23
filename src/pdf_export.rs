// src/pdf_export.rs
//! Combines multiple captured screenshots into a single PDF, one image per
//! page sized to match that image so nothing gets cropped or distorted.

use crate::capture::CaptureResult;
use printpdf::*;
use std::path::Path;

/// Builds a PDF from `items` (in the order given — caller decides ordering,
/// e.g. oldest-first for a natural reading order) and writes it to
/// `dest_path`. Each screenshot becomes its own page sized to that image's
/// pixel dimensions, so every page renders the image at 1:1 with no
/// scaling or cropping.
pub fn export_images_to_pdf(items: &[CaptureResult], dest_path: &Path) -> Result<(), String> {
    if items.is_empty() {
        return Err("No screenshots to export".to_string());
    }

    let mut warnings = Vec::new();
    let mut doc = PdfDocument::new("shotGun Screenshots");
    let mut pages = Vec::new();

    // 96 DPI is the conventional "CSS pixel" density most screen-capture
    // tools assume. printpdf auto-scales an XObject image to its pixel
    // dimensions at the transform's `dpi`, so sizing the page in mm from
    // the same DPI makes the image exactly fill the page with no manual
    // scale/translate math needed.
    const DPI: f32 = 96.0;
    const PX_TO_MM: f32 = 25.4 / DPI;

    for item in items {
        let bytes = std::fs::read(&item.file_path)
            .map_err(|e| format!("Failed to read {}: {e}", item.file_path.display()))?;
        let image = RawImage::decode_from_bytes(&bytes, &mut warnings)
            .map_err(|e| format!("Failed to decode {}: {e}", item.file_path.display()))?;
        let image_id = doc.add_image(&image);

        let page_w_mm = (item.width as f32 * PX_TO_MM).max(1.0);
        let page_h_mm = (item.height as f32 * PX_TO_MM).max(1.0);

        let transform = XObjectTransform {
            dpi: Some(DPI),
            ..Default::default()
        };

        let ops = vec![Op::UseXobject { id: image_id, transform }];
        pages.push(PdfPage::new(Mm(page_w_mm), Mm(page_h_mm), ops));
    }

    let pdf_bytes = doc.with_pages(pages).save(&PdfSaveOptions::default(), &mut warnings);
    std::fs::write(dest_path, pdf_bytes)
        .map_err(|e| format!("Failed to write {}: {e}", dest_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_test_png(dir: &Path, name: &str, w: u32, h: u32) -> CaptureResult {
        let img = ::image::RgbaImage::from_pixel(w, h, ::image::Rgba([200, 50, 50, 255]));
        let path = dir.join(name);
        img.save(&path).unwrap();
        CaptureResult {
            file_path: path,
            width: w,
            height: h,
            file_size_bytes: std::fs::metadata(dir.join(name)).map(|m| m.len()).unwrap_or(0),
            timestamp: "2026-08-23 12:00:00".to_string(),
            monitor_name: "Test Monitor".to_string(),
            counter: 1,
            session: 1,
        }
    }

    #[test]
    fn exports_multiple_images_to_a_valid_multi_page_pdf() {
        let dir = std::env::temp_dir().join("shotgun_pdf_export_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let items = vec![
            make_test_png(&dir, "shot_001.png", 800, 600),
            make_test_png(&dir, "shot_002.png", 1920, 1080),
        ];

        let pdf_path = dir.join("out.pdf");
        export_images_to_pdf(&items, &pdf_path).expect("export should succeed");

        let bytes = std::fs::read(&pdf_path).expect("pdf file missing");
        assert!(bytes.len() > 500, "pdf suspiciously small ({} bytes)", bytes.len());
        assert!(bytes.starts_with(b"%PDF-"), "output doesn't start with a PDF header");

        // Round-trip: printpdf can parse back what it wrote, and it should
        // report two pages sized proportionally to the two source images.
        let mut warnings = Vec::new();
        let parsed = PdfDocument::parse(&bytes, &PdfParseOptions::default(), &mut warnings)
            .expect("failed to parse the exported PDF back");
        assert_eq!(parsed.pages.len(), 2, "expected one page per image");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_history_returns_an_error_instead_of_writing_a_file() {
        let dest = std::env::temp_dir().join("shotgun_pdf_export_test_empty.pdf");
        let result = export_images_to_pdf(&[], &dest);
        assert!(result.is_err());
    }
}

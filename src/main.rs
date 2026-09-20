#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod annotate;
mod app;
mod audio;
mod autostart;
mod capture;
mod config;
mod dxgi_capture;
mod hotkey;
mod overlay;
mod pdf_export;
mod profiles;
mod tray;
mod video;

use app::ShotgunApp;
use eframe::NativeOptions;
use egui::ViewportBuilder;

fn main() -> eframe::Result<()> {
    let native_options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([580.0, 440.0])
            .with_min_inner_size([460.0, 340.0])
            .with_title("shotGun - Screen & Region Capture (Noerotech)")
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "shotGun",
        native_options,
        Box::new(|cc| Ok(Box::new(ShotgunApp::new(cc)))),
    )
}

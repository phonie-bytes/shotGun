#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod config;
mod hotkey;
mod overlay;

use app::ShotgunApp;
use eframe::NativeOptions;
use egui::ViewportBuilder;

fn main() -> eframe::Result<()> {
    let native_options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([740.0, 640.0])
            .with_min_inner_size([560.0, 480.0])
            .with_title("shotGun - Screen & Region Capture")
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "shotGun",
        native_options,
        Box::new(|cc| Ok(Box::new(ShotgunApp::new(cc)))),
    )
}

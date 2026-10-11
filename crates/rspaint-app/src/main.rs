#![forbid(unsafe_code)]
// Hide the console window on Windows release builds (keep it in debug for logs).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod canvas_renderer;
mod clipboard;
mod cursors;
mod text;
mod theme;
mod tools;
mod ui;

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([480.0, 320.0])
            .with_title("RS Paint"),
        ..Default::default()
    };

    eframe::run_native(
        "RS Paint",
        native_options,
        Box::new(|cc| Ok(Box::new(app::PaintApp::new(cc)))),
    )
}

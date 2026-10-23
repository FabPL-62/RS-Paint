#![forbid(unsafe_code)]
// Hide the console window on Windows release builds (keep it in debug for logs).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod canvas_renderer;
mod clipboard;
mod cursors;
mod i18n;
mod text;
mod theme;
mod tool_icons;
mod tools;
mod ui;
mod workspace;

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        // The window is created at this size (its restored size) and the app
        // asks to maximize it on the first frame. Maximizing through the
        // builder is unreliable on Windows: eframe creates the window hidden
        // and winit applies the flag before it is shown, so it can end up
        // restored.
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([480.0, 320.0])
            .with_drag_and_drop(true)
            .with_title("RS Paint"),
        ..Default::default()
    };

    eframe::run_native(
        "RS Paint",
        native_options,
        Box::new(|cc| Ok(Box::new(workspace::Workspace::new(cc)))),
    )
}

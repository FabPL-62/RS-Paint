//! The status bar along the very bottom.

use eframe::egui;

use crate::app::PaintApp;

/// Draws pointer coordinates, canvas size and zoom level.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status-bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let coords = match app.pointer {
                Some((x, y)) => format!("{x}, {y}"),
                None => String::new(),
            };
            ui.label(coords);
            ui.separator();
            ui.label(format!(
                "{} x {}",
                app.document.width(),
                app.document.height()
            ));
            ui.separator();
            ui.label(format!("{}%", (app.magnification * 100.0).round()));
        });
    });
}

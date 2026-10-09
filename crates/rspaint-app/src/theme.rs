//! Visual theme for the app.
//!
//! Phase 0 uses a simple classic-gray look; the full Classic/Modern/Winter
//! themes from JS Paint arrive in a later phase.

use eframe::egui;

/// Applies the default (classic gray) style to the given context.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = egui::Color32::from_rgb(192, 192, 192);
    style.visuals.window_fill = egui::Color32::from_rgb(192, 192, 192);
    style.visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(64, 64, 64));
    style.spacing.item_spacing = egui::vec2(6.0, 6.0);
    ctx.set_style(style);
}

//! Neutral editor surfaces with a restrained blue accent.

use eframe::egui;

pub const PANEL: egui::Color32 = egui::Color32::from_rgb(40, 44, 52);
pub const STRIP: egui::Color32 = egui::Color32::from_rgb(31, 35, 42);
pub const HOVER: egui::Color32 = egui::Color32::from_rgb(52, 58, 69);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(62, 68, 80);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(111, 169, 240);
pub const ICON_SURFACE: egui::Color32 = HOVER;
pub const ICON_SELECTED: egui::Color32 = egui::Color32::from_rgb(49, 76, 109);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.extreme_bg_color = STRIP;
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(49, 76, 109);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.hovered.bg_fill = HOVER;
    style.visuals.widgets.hovered.weak_bg_fill = HOVER;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(170, 179, 193);
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 5.0);
    ctx.set_style(style);
}

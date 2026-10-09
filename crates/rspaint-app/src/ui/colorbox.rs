//! The color box along the bottom.

use eframe::egui;

use crate::app::{to_color32, PaintApp};
use rspaint_core::Color;

/// Draws the current-color indicator and the palette swatches.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("colorbox").show(ctx, |ui| {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            color_indicator(ui, app.foreground, app.background);
            ui.separator();

            let palette = app.palette.clone();
            let custom = app.custom_colors.clone();
            ui.vertical(|ui| {
                for row in palette.chunks(14) {
                    swatch_row(ui, row, app);
                }
                swatch_row(ui, &custom, app);
            });
        });
        ui.add_space(2.0);
    });
}

fn swatch_row(ui: &mut egui::Ui, colors: &[Color], app: &mut PaintApp) {
    ui.horizontal(|ui| {
        for &color in colors {
            let response = swatch(ui, color);
            if response.clicked() {
                app.foreground = color;
            }
            if response.secondary_clicked() {
                app.background = color;
            }
        }
    });
}

fn swatch(ui: &mut egui::Ui, color: Color) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, to_color32(color));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
            egui::StrokeKind::Inside,
        );
    }
    response
}

fn color_indicator(ui: &mut egui::Ui, foreground: Color, background: Color) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(64));
    let back_rect =
        egui::Rect::from_min_size(rect.min + egui::vec2(14.0, 14.0), egui::vec2(28.0, 28.0));
    let front_rect = egui::Rect::from_min_size(rect.min, egui::vec2(28.0, 28.0));
    painter.rect_filled(back_rect, 0.0, to_color32(background));
    painter.rect_stroke(back_rect, 0.0, stroke, egui::StrokeKind::Inside);
    painter.rect_filled(front_rect, 0.0, to_color32(foreground));
    painter.rect_stroke(front_rect, 0.0, stroke, egui::StrokeKind::Inside);
}

//! The color box along the bottom.

use eframe::egui;

use crate::app::{to_color32, ColorTarget, PaintApp};
use rspaint_core::Color;

/// The side length of a palette swatch.
const SWATCH: f32 = 18.0;
/// The gap between swatches (kept small so the palette reads as a grid).
const SWATCH_GAP: f32 = 1.0;
/// The margin left between a swatch and the color picker popup above it.
const PICKER_GAP: f32 = 8.0;
/// How long a swatch must be held for the picker to open.
const HOLD_SECONDS: f64 = 0.4;

/// Draws the current-color indicator and the palette swatches.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("colorbox").show(ctx, |ui| {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            color_indicator(ui, app);
            ui.separator();

            let palette = app.palette.clone();
            let custom = app.custom_colors.clone();
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SWATCH_GAP, SWATCH_GAP);
                for (row_index, row) in palette.chunks(14).enumerate() {
                    swatch_row(ui, row, RowKind::Palette(row_index * 14), app);
                }
                swatch_row(ui, &custom, RowKind::Custom, app);
            });
            ui.separator();
            ui.vertical(|ui| {
                if ui.button("Reset").clicked() {
                    app.reset_colors();
                }
                if ui.button("Capture").clicked() {
                    app.capture_colors();
                }
                ui.horizontal(|ui| {
                    ui.label("Opacity");
                    let mut percent = (f32::from(app.foreground.a) / 255.0 * 100.0).round();
                    if ui
                        .add(
                            egui::DragValue::new(&mut percent)
                                .range(0.0..=100.0)
                                .suffix("%"),
                        )
                        .changed()
                    {
                        app.foreground.a =
                            (percent / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                });
            });
        });
        ui.add_space(2.0);
    });
    // Reset the long-press latch only after processing, so the release that
    // ends a long-press doesn't count as a quick click.
    if !ctx.input(|i| i.pointer.primary_down()) {
        app.swatch_hold_fired = false;
    }
}

/// Which palette section a swatch row belongs to.
#[derive(Clone, Copy)]
enum RowKind {
    Palette(usize),
    Custom,
}

impl RowKind {
    /// The picker target for the swatch at `index` within the row.
    fn target(self, index: usize) -> ColorTarget {
        match self {
            RowKind::Palette(base) => ColorTarget::Palette(base + index),
            RowKind::Custom => ColorTarget::Custom(index),
        }
    }
}

fn swatch_row(ui: &mut egui::Ui, colors: &[Color], kind: RowKind, app: &mut PaintApp) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(SWATCH_GAP, SWATCH_GAP);
        for (index, &color) in colors.iter().enumerate() {
            let response = swatch(ui, color);
            let anchor = response.rect.left_top() - egui::vec2(0.0, PICKER_GAP);

            // Right-click sets the secondary color (no picker).
            if response.secondary_clicked() {
                app.background = color;
            }
            // A quick left-click just picks the primary color (not after a
            // long-press, which only edits the swatch).
            if response.clicked() && !app.swatch_hold_fired {
                app.foreground = color;
            }
            // ...but holding the swatch opens the advanced picker for that slot.
            if response.is_pointer_button_down_on()
                && ui.input(|i| i.pointer.primary_down())
                && !app.swatch_hold_fired
            {
                // Keep repainting while held; otherwise a still pointer would
                // not generate frames and the timer below would never run.
                ui.ctx().request_repaint();
                let elapsed =
                    ui.input(|i| i.pointer.press_start_time().map_or(0.0, |t| i.time - t));
                if elapsed >= HOLD_SECONDS {
                    // Long-press edits the swatch; it does not select it.
                    app.open_color_picker(kind.target(index), color, anchor);
                    app.swatch_hold_fired = true;
                }
            }
        }
    });
}

fn swatch(ui: &mut egui::Ui, color: Color) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SWATCH, SWATCH), egui::Sense::click());
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

fn color_indicator(ui: &mut egui::Ui, app: &mut PaintApp) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(64));
    let back_rect =
        egui::Rect::from_min_size(rect.min + egui::vec2(14.0, 14.0), egui::vec2(28.0, 28.0));
    let front_rect = egui::Rect::from_min_size(rect.min, egui::vec2(28.0, 28.0));
    painter.rect_filled(back_rect, 0.0, to_color32(app.background));
    painter.rect_stroke(back_rect, 0.0, stroke, egui::StrokeKind::Inside);
    painter.rect_filled(front_rect, 0.0, to_color32(app.foreground));
    painter.rect_stroke(front_rect, 0.0, stroke, egui::StrokeKind::Inside);

    // Right-click swaps the primary and secondary colors.
    if response.secondary_clicked() {
        app.swap_colors();
    }
    // Left-click opens the picker for the clicked square (fast, no hold needed).
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let front = egui::Rect::from_min_size(rect.min, egui::vec2(28.0, 28.0));
            let (target, color) = if front.contains(pos) {
                (ColorTarget::Foreground, app.foreground)
            } else {
                (ColorTarget::Background, app.background)
            };
            let anchor = response.rect.left_top() - egui::vec2(0.0, PICKER_GAP);
            app.open_color_picker(target, color, anchor);
        }
    }
}

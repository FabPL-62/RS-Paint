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

const COLUMNS: usize = 14;
const VISIBLE_ROWS: usize = 2;

/// Traditional and captured palettes belong to this document only.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("colorbox").show(ctx, |ui| {
        ui.add_space(2.0);
        egui::ScrollArea::horizontal()
            .id_salt("colorbox-scroll")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    color_indicator(ui, app);
                    ui.add_space(8.0);
                    let palette = app.editor.colors.palette.clone();
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = SWATCH_GAP;
                        for (row_index, row) in
                            palette.chunks(COLUMNS).take(VISIBLE_ROWS).enumerate()
                        {
                            swatch_row(ui, row, RowKind::Palette(row_index * COLUMNS), app);
                        }
                        ui.add_space(8.0);
                        if ui.button(crate::i18n::tr(ui.ctx(), "Reset")).clicked() {
                            app.editor.reset_colors();
                        }
                    });
                    ui.add_space(12.0);
                    captured_palette(app, ui);
                });
            });
        ui.add_space(2.0);
    });
    if !ctx.input(|i| i.pointer.primary_down()) {
        app.swatch_hold_fired = false;
    }
}

fn captured_palette(app: &mut PaintApp, ui: &mut egui::Ui) {
    let max_row = app
        .editor
        .colors
        .captured
        .len()
        .div_ceil(COLUMNS)
        .saturating_sub(VISIBLE_ROWS);
    app.captured_palette_row = app.captured_palette_row.min(max_row);
    let start = app.captured_palette_row * COLUMNS;
    let visible: Vec<_> = (start..start + COLUMNS * VISIBLE_ROWS)
        .map(|index| {
            app.editor
                .colors
                .captured
                .get(index)
                .copied()
                .unwrap_or(Color::BLACK)
        })
        .collect();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = SWATCH_GAP;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = SWATCH_GAP;
                for (row, colors) in visible.chunks(COLUMNS).enumerate() {
                    swatch_row(ui, colors, RowKind::Captured(start + row * COLUMNS), app);
                }
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = SWATCH_GAP;
                ui.spacing_mut().button_padding = egui::vec2(1.0, 0.0);
                let button_size = egui::vec2(SWATCH, SWATCH);
                if ui
                    .add_enabled(
                        app.captured_palette_row > 0,
                        egui::Button::new("▲").min_size(button_size),
                    )
                    .on_hover_text(crate::i18n::tr(ui.ctx(), "Previous palette row"))
                    .clicked()
                {
                    app.captured_palette_row -= 1;
                }
                if ui
                    .add_enabled(
                        app.captured_palette_row < max_row,
                        egui::Button::new("▼").min_size(button_size),
                    )
                    .on_hover_text(crate::i18n::tr(ui.ctx(), "Next palette row"))
                    .clicked()
                {
                    app.captured_palette_row += 1;
                }
            });
        });
        ui.add_space(8.0);
        if ui.button(crate::i18n::tr(ui.ctx(), "Capture")).clicked() {
            app.editor.capture_colors();
            app.captured_palette_row = 0;
        }
    });
}

/// Which palette section a swatch row belongs to.
#[derive(Clone, Copy)]
enum RowKind {
    Palette(usize),
    Captured(usize),
}

impl RowKind {
    /// The picker target for the swatch at `index` within the row.
    fn target(self, index: usize) -> ColorTarget {
        match self {
            RowKind::Palette(base) => ColorTarget::Palette(base + index),
            RowKind::Captured(base) => ColorTarget::Captured(base + index),
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
                app.editor.colors.background = color;
            }
            // A quick left-click just picks the primary color (not after a
            // long-press, which only edits the swatch).
            if response.clicked() && !app.swatch_hold_fired {
                app.editor.colors.foreground = color;
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
    painter.rect_filled(back_rect, 0.0, to_color32(app.editor.colors.background));
    painter.rect_stroke(back_rect, 0.0, stroke, egui::StrokeKind::Inside);
    painter.rect_filled(front_rect, 0.0, to_color32(app.editor.colors.foreground));
    painter.rect_stroke(front_rect, 0.0, stroke, egui::StrokeKind::Inside);

    // Right-click swaps the primary and secondary colors.
    if response.secondary_clicked() {
        app.editor.swap_colors();
    }
    // Left-click opens the picker for the clicked square (fast, no hold needed).
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let front = egui::Rect::from_min_size(rect.min, egui::vec2(28.0, 28.0));
            let (target, color) = if front.contains(pos) {
                (ColorTarget::Foreground, app.editor.colors.foreground)
            } else {
                (ColorTarget::Background, app.editor.colors.background)
            };
            let anchor = response.rect.left_top() - egui::vec2(0.0, PICKER_GAP);
            app.open_color_picker(target, color, anchor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        app: &mut PaintApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1024.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| show(app, ctx),
        )
    }

    fn swatches(output: &egui::FullOutput) -> Vec<egui::Rect> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect.rect.size() == egui::vec2(SWATCH, SWATCH)
                        && rect.corner_radius == egui::CornerRadius::ZERO
                        && rect.fill != egui::Color32::TRANSPARENT =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect()
    }

    fn label_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .expect("palette label exists")
    }

    fn click(ctx: &egui::Context, app: &mut PaintApp, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn palettes_show_two_rows_of_fourteen_with_buttons_below_and_no_opacity_input() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        crate::i18n::set_locale(&ctx, crate::i18n::Locale::English);
        let mut app = PaintApp::new_blank(true, false, &ctx);
        frame(&ctx, &mut app, Vec::new());
        let output = frame(&ctx, &mut app, Vec::new());
        let cells = swatches(&output);
        assert_eq!(cells.len(), 56);
        for palette in cells.chunks(28) {
            for row in palette.chunks(14) {
                assert!(row.iter().all(|rect| rect.top() == row[0].top()));
            }
            assert!(palette[14].top() > palette[0].bottom());
        }
        assert!(cells[28].left() > cells[13].right());
        assert!(label_rect(&output, "Reset").top() >= cells[14].bottom() + 8.0);
        assert!(label_rect(&output, "Capture").top() >= cells[42].bottom() + 8.0);
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Opacity")));
    }

    #[test]
    fn captured_palette_scrolls_by_rows_and_selects_the_visible_absolute_color() {
        let ctx = egui::Context::default();
        let mut app = PaintApp::new_blank(true, false, &ctx);
        app.editor.colors.captured = (0..70).map(|value| Color::rgb(value, 0, 0)).collect();
        app.captured_palette_row = usize::MAX;
        frame(&ctx, &mut app, Vec::new());
        let output = frame(&ctx, &mut app, Vec::new());
        assert_eq!(app.captured_palette_row, 3);
        click(&ctx, &mut app, label_rect(&output, "▲").center());
        assert_eq!(app.captured_palette_row, 2);
        let output = frame(&ctx, &mut app, Vec::new());
        click(&ctx, &mut app, swatches(&output)[31].center());
        assert_eq!(app.editor.colors.foreground, app.editor.colors.captured[31]);
    }
}

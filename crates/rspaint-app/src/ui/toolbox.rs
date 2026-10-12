//! The tool box and tool options on the left.

use eframe::egui;
use rspaint_core::{BlendMode, BrushShape, Color};

use crate::app::{to_color32, BorderPlacement, ColorTarget, FlipRotate, PaintApp, ShapeFill};
use crate::text::FontFamily;
use crate::tools::Tool;

/// Draws the tool selection buttons and the options for the active tool.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::SidePanel::left("toolbox")
        .resizable(false)
        .exact_width(150.0)
        .show(ctx, |ui| {
            // Keep the content inside the fixed panel width: without this, a
            // wide options row grows the panel's measured rect and leaves an
            // unpainted gap between the toolbox and the canvas.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        for tool in Tool::ALL {
                            let mut label = tool.name().to_string();
                            if let Some(shortcut) = tool.shortcut() {
                                label.push_str(&format!("  ({shortcut})"));
                            }
                            let selected = app.tool == tool;
                            let response = ui
                                .add_enabled_ui(tool.implemented(), |ui| {
                                    ui.selectable_label(selected, label)
                                })
                                .inner;
                            if response.clicked() {
                                app.set_tool(tool);
                            }
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(egui::RichText::new("Options").strong());
                        tool_options(app, ui);

                        ui.add_space(6.0);
                        ui.separator();
                        let label = if app.selection.is_some() {
                            "Replace Color (selection)"
                        } else {
                            "Replace Color (all)"
                        };
                        if ui
                            .button(label)
                            .on_hover_text("Replace the foreground color with the background color")
                            .clicked()
                        {
                            app.replace_primary_with_secondary();
                        }
                        if ui
                            .add_enabled(
                                app.selection.is_some(),
                                egui::Button::new("Clear selection"),
                            )
                            .on_hover_text("Deselect the current selection")
                            .clicked()
                        {
                            app.deselect();
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Grid");
                            ui.add(
                                egui::DragValue::new(&mut app.grid_opacity)
                                    .range(0..=100)
                                    .suffix("%"),
                            );
                        });
                    });
                });
        });
}

fn tool_options(app: &mut PaintApp, ui: &mut egui::Ui) {
    match app.tool {
        Tool::Pencil => size_row(ui, "Size", &mut app.pencil_size, 1..=50),
        Tool::Brush => {
            shape_row(ui, &mut app.brush_shape, true);
            size_row(ui, "Size", &mut app.brush_size, 1..=500);
        }
        Tool::SelectionBrush => {
            // Diagonal brush shapes don't make sense for painting a selection.
            shape_row(ui, &mut app.brush_shape, false);
            size_row(ui, "Size", &mut app.brush_size, 1..=500);
            ui.checkbox(&mut app.selection_transparent, "Transparent");
        }
        Tool::Eraser => {
            shape_row(ui, &mut app.eraser_shape, false);
            size_row(ui, "Size", &mut app.eraser_size, 1..=500);
        }
        Tool::Airbrush => size_row(ui, "Size", &mut app.airbrush_size, 1..=500),
        Tool::Line | Tool::Curve => size_row(ui, "Width", &mut app.stroke_size, 1..=500),
        Tool::Polygon => {
            size_row(ui, "Width", &mut app.stroke_size, 1..=500);
            ui.label("Style");
            ui.radio_value(&mut app.fill_style, ShapeFill::Outline, "Outline");
            ui.radio_value(
                &mut app.fill_style,
                ShapeFill::FilledWithOutline,
                "Outline + Fill",
            );
            ui.radio_value(&mut app.fill_style, ShapeFill::FilledOnly, "Fill only");
        }
        Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle => {
            size_row(ui, "Width", &mut app.stroke_size, 1..=500);
            if app.tool == Tool::RoundedRectangle {
                size_row(ui, "Radius", &mut app.round_radius, 0..=200);
            }
            ui.label("Style");
            ui.radio_value(&mut app.fill_style, ShapeFill::Outline, "Outline");
            ui.radio_value(
                &mut app.fill_style,
                ShapeFill::FilledWithOutline,
                "Outline + Fill",
            );
            ui.radio_value(&mut app.fill_style, ShapeFill::FilledOnly, "Fill only");
        }
        Tool::Outliner => {
            size_row(ui, "Size", &mut app.outliner_size, 1..=50);
            ui.label("Position");
            ui.radio_value(
                &mut app.outliner_placement,
                BorderPlacement::Outside,
                "Outside",
            );
            ui.radio_value(
                &mut app.outliner_placement,
                BorderPlacement::OneInward,
                "1px inside",
            );
            ui.radio_value(
                &mut app.outliner_placement,
                BorderPlacement::Symmetric,
                "Symmetric",
            );
            ui.checkbox(&mut app.outliner_concave, "Concave corners");
            if app.outliner_concave {
                ui.horizontal(|ui| {
                    ui.label("Corner color");
                    color_button(ui, app, ColorTarget::Outliner, app.outliner_color);
                });
            }
            ui.label("Drag on the canvas to apply");
        }
        Tool::Text => {
            ui.horizontal(|ui| {
                ui.label("Font");
                egui::ComboBox::from_id_salt("text_family")
                    .selected_text(app.text_family.label())
                    .show_ui(ui, |ui| {
                        for family in FontFamily::ALL {
                            ui.selectable_value(&mut app.text_family, family, family.label());
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Size");
                ui.add(
                    egui::DragValue::new(&mut app.text_size)
                        .range(4.0..=400.0)
                        .speed(0.5),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut app.text_bold, "Bold");
                ui.checkbox(&mut app.text_italic, "Italic");
                ui.checkbox(&mut app.text_underline, "Underline");
            });
            ui.checkbox(&mut app.text_vertical, "Vertical");
            ui.checkbox(&mut app.text_transparent, "Transparent");
            ui.label("Drag on the canvas to place text");
        }
        Tool::Select | Tool::FreeFormSelect | Tool::MagicWand | Tool::ContourSelect => {
            ui.checkbox(&mut app.selection_transparent, "Transparent");
            if app.tool == Tool::ContourSelect {
                ui.label("Uses the secondary color as background");
            }
            if app.tool == Tool::MagicWand {
                ui.horizontal(|ui| {
                    ui.label("Tolerance");
                    let changed = ui
                        .add(egui::DragValue::new(&mut app.magic_wand_tolerance).range(0..=255))
                        .changed();
                    if changed {
                        app.reselect_magic_wand();
                    }
                });
            }
            ui.separator();
            ui.label("Transform selection");
            // These only apply when there is a selection.
            let enabled = app.selection.is_some();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(enabled, egui::Button::new("Flip H"))
                    .clicked()
                {
                    app.transform_selection_direct(FlipRotate::FlipHorizontal);
                }
                if ui
                    .add_enabled(enabled, egui::Button::new("Flip V"))
                    .clicked()
                {
                    app.transform_selection_direct(FlipRotate::FlipVertical);
                }
            });
            ui.horizontal(|ui| {
                for (label, choice) in [
                    ("90°", FlipRotate::Rotate90),
                    ("180°", FlipRotate::Rotate180),
                    ("270°", FlipRotate::Rotate270),
                ] {
                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                        app.transform_selection_direct(choice);
                    }
                }
            });
        }
        _ => {
            ui.label("-");
        }
    }
    if app.tool.uses_blend_mode() {
        ui.separator();
        ui.label("Mode");
        ui.radio_value(&mut app.paint_blend, BlendMode::Blend, "Blend");
        ui.radio_value(&mut app.paint_blend, BlendMode::Replace, "Replace");
    }
}

fn size_row(ui: &mut egui::Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(range).speed(0.5));
    });
}

/// A color swatch button that opens the advanced color picker for `target`.
fn color_button(ui: &mut egui::Ui, app: &mut PaintApp, target: ColorTarget, color: Color) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, to_color32(color));
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
            egui::StrokeKind::Inside,
        );
    }
    if response.clicked() {
        let anchor = response.rect.left_top() - egui::vec2(0.0, 8.0);
        app.open_color_picker(target, color, anchor);
    }
}

fn shape_row(ui: &mut egui::Ui, shape: &mut BrushShape, allow_diagonal: bool) {
    ui.vertical(|ui| {
        ui.radio_value(shape, BrushShape::Circle, "Circle");
        ui.radio_value(shape, BrushShape::Square, "Square");
        if allow_diagonal {
            ui.radio_value(shape, BrushShape::Diagonal, "Diagonal \\");
            ui.radio_value(shape, BrushShape::ReverseDiagonal, "Diagonal /");
        }
    });
}

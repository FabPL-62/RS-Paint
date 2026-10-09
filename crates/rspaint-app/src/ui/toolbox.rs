//! The tool box and tool options on the left.

use eframe::egui;
use rspaint_core::BrushShape;

use crate::app::{PaintApp, ShapeFill};
use crate::tools::Tool;

/// Draws the tool selection buttons and the options for the active tool.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::SidePanel::left("toolbox")
        .resizable(false)
        .exact_width(150.0)
        .show(ctx, |ui| {
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
        }
        Tool::Eraser => size_row(ui, "Size", &mut app.eraser_size, 1..=500),
        Tool::Airbrush => size_row(ui, "Size", &mut app.airbrush_size, 1..=500),
        Tool::Line => size_row(ui, "Width", &mut app.stroke_size, 1..=500),
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
        Tool::Select | Tool::FreeFormSelect | Tool::MagicWand => {
            ui.checkbox(&mut app.selection_transparent, "Transparent");
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
        }
        _ => {
            ui.label("-");
        }
    }
}

fn size_row(ui: &mut egui::Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(range).speed(0.5));
    });
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

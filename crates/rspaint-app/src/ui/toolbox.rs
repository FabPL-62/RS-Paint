//! The tool box and tool options on the left.

use eframe::egui;
use rspaint_core::{BlendMode, BrushShape, Color};

use crate::app::{to_color32, BorderPlacement, ColorTarget, FlipRotate, PaintApp, ShapeFill};
use crate::text::FontFamily;
use crate::tools::Tool;

/// Display order, read left to right in the two-column grid.
const TOOL_ORDER: [Tool; 20] = [
    Tool::FreeFormSelect,
    Tool::Select,
    Tool::MagicWand,
    Tool::ContourSelect,
    Tool::Eraser,
    Tool::SelectionBrush,
    Tool::Fill,
    Tool::Outliner,
    Tool::PickColor,
    Tool::Magnifier,
    Tool::Pencil,
    Tool::Brush,
    Tool::Airbrush,
    Tool::Text,
    Tool::Line,
    Tool::Curve,
    Tool::Rectangle,
    Tool::Polygon,
    Tool::Ellipse,
    Tool::RoundedRectangle,
];

/// Draws the tool selection buttons and the options for the active tool.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::SidePanel::left("toolbox")
        .resizable(false)
        .exact_width(188.0)
        .show(ctx, |ui| {
            // Keep the content inside the fixed panel width: without this, a
            // wide options row grows the panel's measured rect and leaves an
            // unpainted gap between the toolbox and the canvas.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "Tools")).strong());
                        egui::Grid::new("tool-grid")
                            .num_columns(2)
                            .spacing(egui::vec2(8.0, 6.0))
                            .show(ui, |ui| {
                                for (index, tool) in TOOL_ORDER.into_iter().enumerate() {
                                    let texture = app.tool_icons.texture(tool);
                                    // One source pixel per physical screen pixel, including HiDPI.
                                    let icon_size =
                                        texture.size_vec2() / ui.ctx().pixels_per_point();
                                    let image = egui::Image::new((texture.id(), icon_size))
                                        .fit_to_exact_size(icon_size);
                                    let button = egui::Button::image(image)
                                        .min_size(egui::vec2(78.0, 44.0))
                                        .fill(if app.editor.settings.tool == tool {
                                            crate::theme::ICON_SELECTED
                                        } else {
                                            crate::theme::ICON_SURFACE
                                        })
                                        .selected(app.editor.settings.tool == tool);
                                    let response = ui
                                        .push_id(tool.name(), |ui| {
                                            ui.add_enabled(tool.implemented(), button)
                                        })
                                        .inner;
                                    let response = response.on_hover_ui(|ui| {
                                        ui.set_max_width(300.0);
                                        let name = crate::i18n::tr(ui.ctx(), tool.name());
                                        ui.label(egui::RichText::new(name).strong());
                                        ui.label(crate::i18n::tr(
                                            ui.ctx(),
                                            crate::tool_icons::description(tool),
                                        ));
                                        if let Some(shortcut) = tool.shortcut() {
                                            ui.weak(crate::i18n::message(
                                                ui.ctx(),
                                                "Shortcut: {key}",
                                                &[("key", &shortcut.to_string())],
                                            ));
                                        }
                                    });
                                    if response.clicked() {
                                        app.editor.set_tool(tool);
                                    }
                                    if index % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(ui.ctx(), "Options")).strong(),
                        );
                        tool_options(app, ui);

                        ui.add_space(6.0);
                        ui.separator();
                        let label = if app.editor.selection.is_some() {
                            crate::i18n::tr(ui.ctx(), "Replace Color (selection)")
                        } else {
                            crate::i18n::tr(ui.ctx(), "Replace Color (all)")
                        };
                        if ui
                            .button(label)
                            .on_hover_text(crate::i18n::tr(
                                ui.ctx(),
                                "Replace the foreground color with the background color",
                            ))
                            .clicked()
                        {
                            app.editor.replace_primary_with_secondary();
                        }
                        if ui
                            .add_enabled(
                                app.editor.selection.is_some(),
                                egui::Button::new(crate::i18n::tr(ui.ctx(), "Clear selection")),
                            )
                            .on_hover_text(crate::i18n::tr(
                                ui.ctx(),
                                "Deselect the current selection",
                            ))
                            .clicked()
                        {
                            app.editor.deselect();
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr(ui.ctx(), "Grid"));
                            ui.add(
                                egui::DragValue::new(&mut app.editor.settings.grid_opacity)
                                    .range(0..=100)
                                    .suffix("%"),
                            );
                        });
                    });
                });
        });
}

fn tool_options(app: &mut PaintApp, ui: &mut egui::Ui) {
    match app.editor.settings.tool {
        Tool::Pencil => size_row(
            ui,
            crate::i18n::tr(ui.ctx(), "Size"),
            &mut app.editor.settings.pencil_size,
            1..=50,
        ),
        Tool::Brush => {
            shape_row(ui, &mut app.editor.settings.brush_shape, true);
            size_row(
                ui,
                crate::i18n::tr(ui.ctx(), "Size"),
                &mut app.editor.settings.brush_size,
                1..=500,
            );
        }
        Tool::SelectionBrush => {
            // Diagonal brush shapes don't make sense for painting a selection.
            shape_row(ui, &mut app.editor.settings.brush_shape, false);
            size_row(
                ui,
                crate::i18n::tr(ui.ctx(), "Size"),
                &mut app.editor.settings.brush_size,
                1..=500,
            );
            selection_transparency_row(app, ui);
        }
        Tool::Eraser => {
            shape_row(ui, &mut app.editor.settings.eraser_shape, false);
            size_row(
                ui,
                crate::i18n::tr(ui.ctx(), "Size"),
                &mut app.editor.settings.eraser_size,
                1..=500,
            );
        }
        Tool::Airbrush => size_row(
            ui,
            crate::i18n::tr(ui.ctx(), "Size"),
            &mut app.editor.settings.airbrush_size,
            1..=500,
        ),
        Tool::Line | Tool::Curve => size_row(
            ui,
            crate::i18n::tr(ui.ctx(), "Width"),
            &mut app.editor.settings.stroke_size,
            1..=500,
        ),
        Tool::Polygon => {
            size_row(
                ui,
                crate::i18n::tr(ui.ctx(), "Width"),
                &mut app.editor.settings.stroke_size,
                1..=500,
            );
            ui.label(crate::i18n::tr(ui.ctx(), "Style"));
            ui.radio_value(
                &mut app.editor.settings.fill_style,
                ShapeFill::Outline,
                crate::i18n::tr(ui.ctx(), "Outline"),
            );
            ui.radio_value(
                &mut app.editor.settings.fill_style,
                ShapeFill::FilledWithOutline,
                crate::i18n::tr(ui.ctx(), "Outline + Fill"),
            );
            ui.radio_value(
                &mut app.editor.settings.fill_style,
                ShapeFill::FilledOnly,
                crate::i18n::tr(ui.ctx(), "Fill only"),
            );
        }
        Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle => {
            size_row(
                ui,
                crate::i18n::tr(ui.ctx(), "Width"),
                &mut app.editor.settings.stroke_size,
                1..=500,
            );
            if app.editor.settings.tool == Tool::RoundedRectangle {
                size_row(
                    ui,
                    crate::i18n::tr(ui.ctx(), "Radius"),
                    &mut app.editor.settings.round_radius,
                    0..=200,
                );
            }
            ui.label(crate::i18n::tr(ui.ctx(), "Style"));
            ui.radio_value(
                &mut app.editor.settings.fill_style,
                ShapeFill::Outline,
                crate::i18n::tr(ui.ctx(), "Outline"),
            );
            ui.radio_value(
                &mut app.editor.settings.fill_style,
                ShapeFill::FilledWithOutline,
                crate::i18n::tr(ui.ctx(), "Outline + Fill"),
            );
            ui.radio_value(
                &mut app.editor.settings.fill_style,
                ShapeFill::FilledOnly,
                crate::i18n::tr(ui.ctx(), "Fill only"),
            );
        }
        Tool::Outliner => {
            size_row(
                ui,
                crate::i18n::tr(ui.ctx(), "Size"),
                &mut app.editor.settings.outliner_size,
                1..=50,
            );
            ui.label(crate::i18n::tr(ui.ctx(), "Position"));
            ui.radio_value(
                &mut app.editor.settings.outliner_placement,
                BorderPlacement::Outside,
                crate::i18n::tr(ui.ctx(), "Outside"),
            );
            ui.radio_value(
                &mut app.editor.settings.outliner_placement,
                BorderPlacement::OneInward,
                crate::i18n::tr(ui.ctx(), "1px inside"),
            );
            ui.radio_value(
                &mut app.editor.settings.outliner_placement,
                BorderPlacement::Symmetric,
                crate::i18n::tr(ui.ctx(), "Symmetric"),
            );
            ui.checkbox(
                &mut app.editor.settings.outliner_concave,
                crate::i18n::tr(ui.ctx(), "Concave corners"),
            );
            if app.editor.settings.outliner_concave {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr(ui.ctx(), "Corner color"));
                    color_button(
                        ui,
                        app,
                        ColorTarget::Outliner,
                        app.editor.settings.outliner_color,
                    );
                });
            }
            ui.label(crate::i18n::tr(ui.ctx(), "Drag on the canvas to apply"));
        }
        Tool::Text => {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr(ui.ctx(), "Font"));
                egui::ComboBox::from_id_salt("text_family")
                    .selected_text(crate::i18n::tr(
                        ui.ctx(),
                        app.editor.settings.text_family.label(),
                    ))
                    .show_ui(ui, |ui| {
                        for family in FontFamily::ALL {
                            ui.selectable_value(
                                &mut app.editor.settings.text_family,
                                family,
                                crate::i18n::tr(ui.ctx(), family.label()),
                            );
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr(ui.ctx(), "Size"));
                ui.add(
                    egui::DragValue::new(&mut app.editor.settings.text_size)
                        .range(4.0..=400.0)
                        .speed(0.5),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(
                    &mut app.editor.settings.text_bold,
                    crate::i18n::tr(ui.ctx(), "Bold"),
                );
                ui.checkbox(
                    &mut app.editor.settings.text_italic,
                    crate::i18n::tr(ui.ctx(), "Italic"),
                );
                ui.checkbox(
                    &mut app.editor.settings.text_underline,
                    crate::i18n::tr(ui.ctx(), "Underline"),
                );
            });
            ui.checkbox(
                &mut app.editor.settings.text_vertical,
                crate::i18n::tr(ui.ctx(), "Vertical"),
            );
            ui.checkbox(
                &mut app.editor.settings.text_transparent,
                crate::i18n::tr(ui.ctx(), "Transparent"),
            );
            ui.label(crate::i18n::tr(
                ui.ctx(),
                "Drag on the canvas to place text",
            ));
        }
        Tool::Select | Tool::FreeFormSelect | Tool::MagicWand | Tool::ContourSelect => {
            selection_transparency_row(app, ui);
            if app.editor.settings.tool == Tool::ContourSelect {
                ui.label(crate::i18n::tr(
                    ui.ctx(),
                    "Uses the secondary color as background",
                ));
            }
            if app.editor.settings.tool == Tool::MagicWand {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr(ui.ctx(), "Tolerance"));
                    let changed = ui
                        .add(
                            egui::DragValue::new(&mut app.editor.settings.magic_wand_tolerance)
                                .range(0..=255),
                        )
                        .changed();
                    if changed {
                        app.editor.reselect_magic_wand();
                    }
                });
            }
            ui.separator();
            ui.label(crate::i18n::tr(ui.ctx(), "Transform selection"));
            // These only apply when there is a selection.
            let enabled = app.editor.selection.is_some();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(crate::i18n::tr(ui.ctx(), "Flip H")),
                    )
                    .clicked()
                {
                    app.editor
                        .transform_selection_direct(FlipRotate::FlipHorizontal);
                }
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(crate::i18n::tr(ui.ctx(), "Flip V")),
                    )
                    .clicked()
                {
                    app.editor
                        .transform_selection_direct(FlipRotate::FlipVertical);
                }
            });
            ui.horizontal(|ui| {
                for (label, choice) in [
                    ("90°", FlipRotate::Rotate90),
                    ("180°", FlipRotate::Rotate180),
                    ("270°", FlipRotate::Rotate270),
                ] {
                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                        app.editor.transform_selection_direct(choice);
                    }
                }
            });
        }
        _ => {
            ui.label("-");
        }
    }
    if app.editor.settings.tool.uses_blend_mode() {
        ui.separator();
        ui.label(crate::i18n::tr(ui.ctx(), "Mode"));
        ui.radio_value(
            &mut app.editor.settings.paint_blend,
            BlendMode::Blend,
            crate::i18n::tr(ui.ctx(), "Blend"),
        );
        ui.radio_value(
            &mut app.editor.settings.paint_blend,
            BlendMode::Replace,
            crate::i18n::tr(ui.ctx(), "Replace"),
        );
    }
}

fn size_row(ui: &mut egui::Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(range).speed(0.5));
    });
}

fn selection_transparency_row(app: &mut PaintApp, ui: &mut egui::Ui) {
    let mut transparent = app.editor.settings.selection_transparent;
    if ui
        .checkbox(&mut transparent, crate::i18n::tr(ui.ctx(), "Transparent"))
        .changed()
    {
        app.editor.toggle_selection_transparency();
    }
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
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(80)),
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
        ui.radio_value(
            shape,
            BrushShape::Circle,
            crate::i18n::tr(ui.ctx(), "Circle"),
        );
        ui.radio_value(
            shape,
            BrushShape::Square,
            crate::i18n::tr(ui.ctx(), "Square"),
        );
        if allow_diagonal {
            ui.radio_value(
                shape,
                BrushShape::Diagonal,
                crate::i18n::tr(ui.ctx(), "Diagonal \\"),
            );
            ui.radio_value(
                shape,
                BrushShape::ReverseDiagonal,
                crate::i18n::tr(ui.ctx(), "Diagonal /"),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_images_form_two_columns_and_ten_rows() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut app = PaintApp::new_blank(true, false, &ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1024.0, 768.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input.clone(), |ctx| show(&mut app, ctx));
        let output = ctx.run(input, |ctx| show(&mut app, ctx));
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let mut rectangles = Vec::new();
        for tool in TOOL_ORDER {
            let id = app.tool_icons.texture(tool).id();
            let rect = primitives
                .iter()
                .find_map(|primitive| match &primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == id => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .expect("each tool has a visible image");
            rectangles.push(rect);
        }
        for row in rectangles.chunks(2) {
            assert_eq!(row[0].center().y, row[1].center().y);
            assert!(row[0].right() < row[1].left());
            assert!(row[1].right() <= 188.0);
        }
        for rows in rectangles.chunks(2).collect::<Vec<_>>().windows(2) {
            assert!(rows[0][0].bottom() < rows[1][0].top());
            assert_eq!(rows[0][0].center().x, rows[1][0].center().x);
            assert_eq!(rows[0][1].center().x, rows[1][1].center().x);
        }
    }
}

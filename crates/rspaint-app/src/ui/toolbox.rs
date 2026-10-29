//! The single-column tool box on the left and the tool options sidebar on the
//! right.

use eframe::egui;
use rspaint_core::{BlendMode, BrushShape, Color};

use crate::app::{to_color32, BorderPlacement, ColorTarget, FlipRotate, PaintApp, ShapeFill};
use crate::text::FontFamily;
use crate::tools::Tool;

/// Display order, read top to bottom in the single-column tool box.
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

/// Side of a tool button, in points. The icon keeps its native pixel size and
/// is centered inside it, so every button is a perfect square.
const TOOL_BUTTON: f32 = 31.0;
/// Inner margin of the tool box around the buttons.
const TOOLBOX_MARGIN: f32 = 7.0;
/// Tool box width: the buttons plus both margins.
const TOOLBOX_WIDTH: f32 = TOOL_BUTTON + 2.0 * TOOLBOX_MARGIN;
/// Tool options sidebar width.
const OPTIONS_WIDTH: f32 = 200.0;

/// Draws the tool selection buttons in a single column.
pub fn show_tools(app: &mut PaintApp, ctx: &egui::Context) {
    egui::SidePanel::left("toolbox")
        .resizable(false)
        .exact_width(TOOLBOX_WIDTH)
        .frame(
            egui::Frame::new()
                .fill(crate::theme::PANEL)
                .inner_margin(egui::Margin::symmetric(TOOLBOX_MARGIN as i8, 4)),
        )
        .show(ctx, |ui| {
            // Hide the bar so the square buttons keep the full panel width;
            // the wheel still scrolls when the column does not fit.
            egui::ScrollArea::vertical()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);
                    ui.add_space(4.0);
                    for tool in TOOL_ORDER {
                        tool_button(app, ui, tool);
                    }
                    ui.add_space(4.0);
                });
        });
}

/// Draws the options for the active tool in a right-hand sidebar. With no
/// document there is no active tool, so the sidebar stays hidden.
pub fn show_options(app: &mut PaintApp, ctx: &egui::Context) {
    if app.empty_host() {
        return;
    }
    egui::SidePanel::right("tool-options")
        .resizable(false)
        .exact_width(OPTIONS_WIDTH)
        .show(ctx, |ui| {
            // Keep the content inside the fixed panel width: without this, a
            // wide options row grows the panel's measured rect and leaves an
            // unpainted gap between the canvas and the sidebar.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "Options")).strong());
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
                        .on_hover_text(crate::i18n::tr(ui.ctx(), "Deselect the current selection"))
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
}

/// A square tool button with the icon painted at its native size.
fn tool_button(app: &mut PaintApp, ui: &mut egui::Ui, tool: Tool) -> egui::Response {
    // With no document the tools are inert: dim the icons and drop the clicks.
    let enabled = !app.empty_host() && tool.implemented();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(TOOL_BUTTON, TOOL_BUTTON), egui::Sense::click());
    let selected = app.editor.settings.tool == tool;
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            2.0,
            if selected {
                crate::theme::ICON_SELECTED
            } else {
                crate::theme::ICON_SURFACE
            },
        );
        if enabled && response.hovered() {
            painter.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0_f32, crate::theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let texture = app.tool_icons.texture(tool);
        // One source pixel per physical screen pixel, including HiDPI. The
        // icon is centered without ever being resized.
        let icon_size = texture.size_vec2() / ui.ctx().pixels_per_point();
        let icon_rect = egui::Rect::from_center_size(rect.center(), icon_size);
        painter.image(
            texture.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            if enabled {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_gray(120)
            },
        );
    }
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
    if enabled && response.clicked() {
        app.editor.set_tool(tool);
    }
    response
}

fn tool_options(app: &mut PaintApp, ui: &mut egui::Ui) {
    match app.editor.settings.tool {
        Tool::Pencil => {
            size_row(
                ui,
                crate::i18n::tr(ui.ctx(), "Size"),
                &mut app.editor.settings.pencil_size,
                1..=50,
            );
            smoothing_row(app, ui);
            let size = app.editor.settings.pencil_size;
            pixel_perfect_row(app, ui, size);
        }
        Tool::Brush => {
            shape_row(ui, &mut app.editor.settings.brush_shape, true);
            size_row(
                ui,
                crate::i18n::tr(ui.ctx(), "Size"),
                &mut app.editor.settings.brush_size,
                1..=500,
            );
            smoothing_row(app, ui);
            let size = app.editor.settings.brush_size;
            pixel_perfect_row(app, ui, size);
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

/// Pointer smoothing percentage for the Pencil and Brush tools.
fn smoothing_row(app: &mut PaintApp, ui: &mut egui::Ui) {
    let mut smoothing = app.editor.settings.stroke_smoothing;
    ui.vertical(|ui| {
        ui.label(crate::i18n::tr(ui.ctx(), "Smoothing"));
        ui.add(egui::Slider::new(&mut smoothing, 0..=100).suffix("%"))
            .on_hover_text(crate::i18n::tr(
                ui.ctx(),
                "Higher values make the stroke follow the pointer more slowly and smoothly",
            ));
    });
    if smoothing != app.editor.settings.stroke_smoothing {
        app.editor.settings.stroke_smoothing = smoothing;
    }
}

/// Toggles pixel-perfect corner removal. Only 1 px strokes are affected, so
/// the option is disabled for larger sizes.
fn pixel_perfect_row(app: &mut PaintApp, ui: &mut egui::Ui, size: u32) {
    let mut pixel_perfect = app.editor.settings.pixel_perfect;
    ui.add_enabled(
        size == 1,
        egui::Checkbox::new(
            &mut pixel_perfect,
            crate::i18n::tr(ui.ctx(), "Pixel-perfect corners"),
        ),
    )
    .on_hover_text(crate::i18n::tr(
        ui.ctx(),
        "Removes the extra pixel at the corners of 1 px strokes",
    ));
    if pixel_perfect != app.editor.settings.pixel_perfect {
        app.editor.settings.pixel_perfect = pixel_perfect;
    }
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

    fn frame() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1024.0, 768.0),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn tool_images_form_a_single_column_at_native_size() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut app = PaintApp::new_blank(true, false, &ctx);
        let _ = ctx.run(frame(), |ctx| show_tools(&mut app, ctx));
        let output = ctx.run(frame(), |ctx| show_tools(&mut app, ctx));
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let mut rectangles = Vec::new();
        for tool in TOOL_ORDER {
            let texture = app.tool_icons.texture(tool);
            let id = texture.id();
            let rect = primitives
                .iter()
                .find_map(|primitive| match &primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == id => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .expect("each tool has a visible image");
            // The icon is never scaled: its mesh keeps the texture's size.
            let native = texture.size_vec2() / output.pixels_per_point;
            assert!((rect.width() - native.x).abs() < 0.01, "{tool:?} width");
            assert!((rect.height() - native.y).abs() < 0.01, "{tool:?} height");
            rectangles.push(rect);
        }
        // One column, stacked top to bottom inside the narrow tool box.
        for pair in rectangles.windows(2) {
            assert_eq!(pair[0].center().x, pair[1].center().x);
            assert!(pair[0].bottom() < pair[1].top());
            assert!(pair[0].right() <= TOOLBOX_WIDTH);
        }
    }

    #[test]
    fn tool_buttons_are_small_squares() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut app = PaintApp::new_blank(true, false, &ctx);
        let mut rects = Vec::new();
        let _ = ctx.run(frame(), |ctx| {
            egui::SidePanel::left("probe")
                .exact_width(TOOLBOX_WIDTH)
                .show(ctx, |ui| {
                    for tool in TOOL_ORDER {
                        rects.push(tool_button(&mut app, ui, tool).rect);
                    }
                });
        });
        assert_eq!(rects.len(), TOOL_ORDER.len());
        for rect in rects {
            assert_eq!(rect.width(), TOOL_BUTTON);
            assert_eq!(rect.height(), TOOL_BUTTON);
        }
    }

    #[test]
    fn options_sidebar_only_shows_with_a_document() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut app = PaintApp::new_blank(true, false, &ctx);
        let mut available = Vec::new();
        for empty in [false, true] {
            app.set_empty_host(empty);
            let mut width = 0.0;
            let _ = ctx.run(frame(), |ctx| {
                show_tools(&mut app, ctx);
                show_options(&mut app, ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    width = ui.available_width();
                });
            });
            available.push(width);
        }
        // Hiding the sidebar gives the extra width back to the canvas, which
        // is what the pan limits follow.
        assert_eq!(available[1] - available[0], OPTIONS_WIDTH);
    }
}

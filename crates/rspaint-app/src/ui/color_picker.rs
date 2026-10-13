//! The advanced HSV color picker popup.

use eframe::egui;

use crate::app::{to_color32, ColorTarget, PaintApp};
use rspaint_core::Color;

/// The size of the saturation/value square.
const SQUARE: egui::Vec2 = egui::vec2(220.0, 160.0);
/// The width of the vertical hue strip.
const HUE_WIDTH: f32 = 22.0;
/// The height of the horizontal value bar.
const VALUE_HEIGHT: f32 = 18.0;

/// Draws the color picker popup, if open.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    let Some(picker) = app.color_picker_ref() else {
        return;
    };
    let mut hue = picker.hue;
    let mut sat = picker.sat;
    let mut val = picker.val;
    let mut alpha = picker.alpha;
    let anchor = picker.anchor;
    let just_opened = picker.just_opened;
    let target = picker.target;

    let mut open = true;
    let inner = egui::Area::new(egui::Id::new("color_picker"))
        .fixed_pos(anchor)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let top_row = ui.horizontal(|ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(SQUARE, egui::Sense::click_and_drag());
                    ui.painter().add(egui::Shape::mesh(square_mesh(rect, hue)));
                    if response.is_pointer_button_down_on() || response.dragged() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            sat = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                            val = 1.0 - ((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
                        }
                    }
                    let handle =
                        rect.min + egui::vec2(sat * rect.width(), (1.0 - val) * rect.height());
                    draw_handle(ui.painter(), handle);

                    let (hue_rect, hue_response) = ui.allocate_exact_size(
                        egui::vec2(HUE_WIDTH, SQUARE.y),
                        egui::Sense::click_and_drag(),
                    );
                    ui.painter().add(egui::Shape::mesh(hue_mesh(hue_rect)));
                    if hue_response.is_pointer_button_down_on() || hue_response.dragged() {
                        if let Some(pos) = hue_response.interact_pointer_pos() {
                            hue = ((pos.y - hue_rect.min.y) / hue_rect.height()).clamp(0.0, 1.0)
                                * 360.0;
                        }
                    }
                    let hy = hue_rect.min.y + (hue / 360.0) * hue_rect.height();
                    ui.painter().line_segment(
                        [
                            egui::pos2(hue_rect.min.x, hy),
                            egui::pos2(hue_rect.max.x, hy),
                        ],
                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                    );
                });

                // Match the value bar to the width of the square + hue strip.
                let bar_width = top_row.response.rect.width();
                let (value_rect, value_response) = ui.allocate_exact_size(
                    egui::vec2(bar_width, VALUE_HEIGHT),
                    egui::Sense::click_and_drag(),
                );
                ui.painter()
                    .add(egui::Shape::mesh(value_mesh(value_rect, hue, sat)));
                if value_response.is_pointer_button_down_on() || value_response.dragged() {
                    if let Some(pos) = value_response.interact_pointer_pos() {
                        val = ((pos.x - value_rect.min.x) / value_rect.width()).clamp(0.0, 1.0);
                    }
                }
                let vx = value_rect.min.x + val * value_rect.width();
                ui.painter().line_segment(
                    [
                        egui::pos2(vx, value_rect.min.y),
                        egui::pos2(vx, value_rect.max.y),
                    ],
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                );

                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr(ui.ctx(), "Opacity"));
                    ui.add(egui::Slider::new(&mut alpha, 0.0..=1.0).show_value(false));
                    ui.label(format!("{}%", (alpha * 100.0).round() as i32));
                });
                ui.horizontal(|ui| {
                    let color = Color::from_hsv(hue, sat, val);
                    let (preview, _) =
                        ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                    ui.painter().rect_filled(preview, 2.0, to_color32(color));
                    ui.painter().rect_stroke(
                        preview,
                        2.0,
                        egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
                        egui::StrokeKind::Inside,
                    );
                    ui.label(format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b));
                    ui.label(format!("RGB {} {} {}", color.r, color.g, color.b));
                    if ui.button(crate::i18n::tr(ui.ctx(), "Close")).clicked() {
                        open = false;
                    }
                });
                ui.label(match target {
                    ColorTarget::Foreground => crate::i18n::tr(ui.ctx(), "Primary color"),
                    ColorTarget::Background => crate::i18n::tr(ui.ctx(), "Secondary color"),
                    ColorTarget::Palette(_) => crate::i18n::tr(ui.ctx(), "Palette color"),
                    ColorTarget::Custom(_) => crate::i18n::tr(ui.ctx(), "Recent color"),
                    ColorTarget::Outliner => crate::i18n::tr(ui.ctx(), "Corner color"),
                });
            });
        });

    // Close when a new press starts outside the popup. Using the press (not the
    // click) means releasing the long-press that opened it doesn't close it.
    if open && !just_opened {
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !inner.response.rect.contains(p))
        });
        if pressed_outside {
            open = false;
        }
    }

    if open {
        if let Some(picker) = app.color_picker_mut() {
            picker.hue = hue;
            picker.sat = sat;
            picker.val = val;
            picker.alpha = alpha;
            picker.just_opened = false;
        }
        app.apply_color_picker();
    } else {
        app.close_color_picker();
    }
    ctx.request_repaint();
}

/// Builds the saturation (x) / value (y) gradient mesh for `hue`.
fn square_mesh(rect: egui::Rect, hue: f32) -> egui::Mesh {
    let n = 24;
    let mut mesh = egui::Mesh::default();
    for iy in 0..=n {
        for ix in 0..=n {
            let fx = ix as f32 / n as f32;
            let fy = iy as f32 / n as f32;
            let color = Color::from_hsv(hue, fx, 1.0 - fy);
            let x = rect.min.x + (rect.max.x - rect.min.x) * fx;
            let y = rect.min.y + (rect.max.y - rect.min.y) * fy;
            mesh.colored_vertex(egui::pos2(x, y), to_color32(color));
        }
    }
    let stride = (n + 1) as u32;
    for iy in 0..n as u32 {
        for ix in 0..n as u32 {
            let i = iy * stride + ix;
            mesh.add_triangle(i, i + 1, i + stride);
            mesh.add_triangle(i + 1, i + stride + 1, i + stride);
        }
    }
    mesh
}

/// Builds the vertical hue gradient mesh.
fn hue_mesh(rect: egui::Rect) -> egui::Mesh {
    let n = 32;
    let mut mesh = egui::Mesh::default();
    for i in 0..=n {
        let f = i as f32 / n as f32;
        let color = to_color32(Color::from_hsv(f * 360.0, 1.0, 1.0));
        let y = rect.min.y + (rect.max.y - rect.min.y) * f;
        mesh.colored_vertex(egui::pos2(rect.min.x, y), color);
        mesh.colored_vertex(egui::pos2(rect.max.x, y), color);
    }
    for i in 0..n as u32 {
        let a = i * 2;
        mesh.add_triangle(a, a + 2, a + 1);
        mesh.add_triangle(a + 1, a + 2, a + 3);
    }
    mesh
}

/// Builds the horizontal value gradient mesh (black to the pure color).
fn value_mesh(rect: egui::Rect, hue: f32, sat: f32) -> egui::Mesh {
    let n = 24;
    let mut mesh = egui::Mesh::default();
    for i in 0..=n {
        let f = i as f32 / n as f32;
        let color = to_color32(Color::from_hsv(hue, sat, f));
        let x = rect.min.x + (rect.max.x - rect.min.x) * f;
        mesh.colored_vertex(egui::pos2(x, rect.min.y), color);
        mesh.colored_vertex(egui::pos2(x, rect.max.y), color);
    }
    for i in 0..n as u32 {
        let a = i * 2;
        mesh.add_triangle(a, a + 2, a + 1);
        mesh.add_triangle(a + 1, a + 2, a + 3);
    }
    mesh
}

/// Draws a circular handle with a contrasting outline.
fn draw_handle(painter: &egui::Painter, center: egui::Pos2) {
    painter.circle_stroke(center, 6.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
    painter.circle_stroke(center, 7.0, egui::Stroke::new(1.0, egui::Color32::BLACK));
}

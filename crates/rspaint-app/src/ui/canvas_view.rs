//! The pan/zoom canvas viewport and pointer interaction.
//!
//! The document is drawn by a custom wgpu render pass (see
//! [`crate::canvas_renderer`]) which also composites the transparency
//! checkerboard, the pixel grid and the tool cursor on the GPU.
//!
//! Navigation:
//! - Middle mouse drag pans the view.
//! - Ctrl + wheel zooms in/out, anchored at the pointer.
//! - Wheel pans vertically, Shift + wheel pans horizontally.

use eframe::egui;
use eframe::egui_wgpu;
use egui::{PointerButton, Pos2, Vec2};

use crate::app::{
    to_color32, PaintApp, PickTarget, SelectionOutline, MAX_MAGNIFICATION, MIN_MAGNIFICATION,
};
use crate::canvas_renderer::CanvasCallback;
use crate::tools::Tool;

/// Draws the current document and routes pointer input to the active tool.
pub fn show(app: &mut PaintApp, ui: &mut egui::Ui) {
    if !app.has_gpu() {
        ui.label("The GPU renderer is unavailable.");
        return;
    }

    let viewport_size = ui.available_size();
    let (viewport_rect, response) =
        ui.allocate_exact_size(viewport_size, egui::Sense::click_and_drag());

    handle_navigation(app, ui, viewport_rect, viewport_size);

    let magnification = app.magnification;
    let doc_size = egui::vec2(
        app.document.width() as f32 * magnification,
        app.document.height() as f32 * magnification,
    );
    let canvas_rect = egui::Rect::from_min_size(viewport_rect.min + app.pan, doc_size);

    let to_canvas = |pos: Pos2| -> (i32, i32) {
        (
            ((pos.x - canvas_rect.min.x) / magnification).floor() as i32,
            ((pos.y - canvas_rect.min.y) / magnification).floor() as i32,
        )
    };

    // Report pointer coordinates only while over the canvas.
    let pointer = response.hover_pos().and_then(|pos| {
        let p = to_canvas(pos);
        let inside = p.0 >= 0
            && p.1 >= 0
            && p.0 < app.document.width() as i32
            && p.1 < app.document.height() as i32;
        inside.then_some(p)
    });
    app.set_pointer(pointer);

    if let Some(target) = app.pick_hold() {
        handle_pick(app, &response, canvas_rect, &to_canvas, target);
    } else if app.tool == Tool::MagicWand {
        handle_magic_wand(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.tool == Tool::SelectionBrush {
        handle_selection_brush(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.tool.is_selection() {
        handle_selection(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.tool == Tool::Polygon {
        handle_polygon(app, &response, canvas_rect, &to_canvas);
    } else if app.tool == Tool::Curve {
        handle_curve(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.tool == Tool::Text {
        handle_text(app, ui, &response, canvas_rect, &to_canvas);
    } else {
        handle_drawing(app, ui, &response, canvas_rect, &to_canvas);
    }

    let visible = canvas_rect.intersect(viewport_rect);
    if visible.width() > 0.0 && visible.height() > 0.0 {
        let uv_rect = visible_uv_rect(canvas_rect, visible);
        let uniforms = app.canvas_uniforms(magnification, ui.ctx().pixels_per_point(), uv_rect);
        let upload = app.take_pending_upload();
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            visible,
            CanvasCallback { uniforms, upload },
        ));
    }

    paint_selection_overlay(app, ui, canvas_rect, magnification);
    paint_polygon_overlay(app, ui, canvas_rect, magnification);
    paint_text_overlay(app, ui, canvas_rect, magnification);
}

/// Handles the temporary Ctrl/Alt color-pick while a drawing tool is active.
fn handle_pick(
    app: &mut PaintApp,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
    target: PickTarget,
) {
    if (response.clicked() || response.dragged())
        && response
            .interact_pointer_pos()
            .is_some_and(|pos| canvas_rect.contains(pos))
    {
        if let Some(pos) = response.interact_pointer_pos() {
            app.pick_color_at(to_canvas(pos), target);
        }
    }
}

fn handle_polygon(
    app: &mut PaintApp,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);
    app.set_polygon_preview(
        response
            .hover_pos()
            .filter(|&pos| on_canvas(pos))
            .map(to_canvas),
    );

    if response.double_clicked() {
        app.finish_polygon();
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.polygon_click(to_canvas(pos));
            }
        }
    }
    if response.secondary_clicked() {
        app.finish_polygon();
    }
}

/// Handles the multi-step Curve tool: draw a line, then drag two control points.
fn handle_curve(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);

    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if on_canvas(pos) {
                app.curve_pointer_down(to_canvas(pos));
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.curve_pointer_move(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.curve_pointer_up();
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.curve_pointer_down(to_canvas(pos));
                app.curve_pointer_up();
            }
        }
    }
    if response.secondary_clicked() {
        app.curve_cancel();
    }
}

/// Handles the Text tool: drag to place a box, type, and click outside to commit.
fn handle_text(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);

    if app.has_text_box() {
        // Capture typing: append characters, delete with Backspace, newline
        // with Enter. (Global single-key shortcuts are suppressed meanwhile.)
        let (typed, backspace, newline) = ui.ctx().input_mut(|i| {
            let mut typed = String::new();
            for event in &i.events {
                if let egui::Event::Text(text) = event {
                    typed.push_str(text);
                }
            }
            let backspace = i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace);
            let newline = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            (typed, backspace, newline)
        });
        if !typed.is_empty() {
            app.text_type(&typed);
        }
        if backspace {
            app.text_backspace();
        }
        if newline {
            app.text_newline();
        }
        app.render_text_preview();

        // A click outside the box commits the text.
        if response.clicked() || response.secondary_clicked() {
            app.commit_text();
        }
        return;
    }

    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if on_canvas(pos) {
                app.begin_text_box(to_canvas(pos));
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.update_text_box(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.finish_text_box();
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.default_text_box(to_canvas(pos));
            }
        }
    }
    if response.secondary_clicked() {
        app.cancel_text();
    }
}

/// Draws the text box being created and hosts the editor for the active box.
fn paint_text_overlay(
    app: &mut PaintApp,
    ui: &mut egui::Ui,
    canvas_rect: egui::Rect,
    magnification: f32,
) {
    if let Some((a, b)) = app.text_creating_rect() {
        let min = canvas_rect.min
            + egui::vec2(
                a.0.min(b.0) as f32 * magnification,
                a.1.min(b.1) as f32 * magnification,
            );
        let max = canvas_rect.min
            + egui::vec2(
                (a.0.max(b.0) + 1) as f32 * magnification,
                (a.1.max(b.1) + 1) as f32 * magnification,
            );
        let poly = [
            min,
            egui::pos2(max.x, min.y),
            max,
            egui::pos2(min.x, max.y),
            min,
        ];
        paint_ants(ui.painter(), &poly, 0.0);
        return;
    }

    let Some((x, y, width, height)) = app.text_box_rect() else {
        return;
    };

    let min = canvas_rect.min + egui::vec2(x as f32 * magnification, y as f32 * magnification);
    let rect = egui::Rect::from_min_size(
        min,
        egui::vec2(width as f32 * magnification, height as f32 * magnification),
    );
    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(128)),
        egui::StrokeKind::Outside,
    );

    // Blinking caret at the end of the text.
    let time = ui.input(|i| i.time);
    if (time * 2.0) as i64 % 2 == 0 {
        if let Some((a, b)) = app.text_caret_segment() {
            let p0 = min + egui::vec2(a.0 * magnification, a.1 * magnification);
            let p1 = min + egui::vec2(b.0 * magnification, b.1 * magnification);
            ui.painter()
                .line_segment([p0, p1], egui::Stroke::new(1.0, to_color32(app.foreground)));
        }
    }
    ui.ctx().request_repaint();
}

/// Draws the in-progress polygon outline and the rubber-band preview.
fn paint_polygon_overlay(
    app: &PaintApp,
    ui: &egui::Ui,
    canvas_rect: egui::Rect,
    magnification: f32,
) {
    if app.polygon_points().is_empty() {
        return;
    }
    let to_screen = |(x, y): (i32, i32)| -> Pos2 {
        canvas_rect.min
            + egui::vec2(
                (x as f32 + 0.5) * magnification,
                (y as f32 + 0.5) * magnification,
            )
    };
    let mut poly: Vec<Pos2> = app.polygon_points().iter().map(|&p| to_screen(p)).collect();
    if let Some(preview) = app.polygon_preview() {
        poly.push(to_screen(preview));
    }
    let black = egui::Stroke::new(1.0, egui::Color32::BLACK);
    let white = egui::Stroke::new(1.0, egui::Color32::WHITE);
    let painter = ui.painter();
    for pair in poly.windows(2) {
        let offset = egui::vec2(1.0, 1.0);
        painter.line_segment([pair[0] + offset, pair[1] + offset], white);
        painter.line_segment([pair[0], pair[1]], black);
    }
    ui.ctx().request_repaint();
}

fn handle_selection(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let tool = app.tool;
    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);

    if response.drag_started() && ui.input(|i| i.pointer.primary_down()) {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if on_canvas(pos) {
                app.begin_selection_drag(to_canvas(pos), tool, shift, alt);
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.update_selection_drag(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.end_selection_drag();
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let p = to_canvas(pos);
            if shift && on_canvas(pos) {
                // Shift + tap adds a single pixel to the selection.
                app.select_pixel(p, true, false);
            } else if alt && on_canvas(pos) {
                // Alt + tap removes a single pixel from the selection.
                app.select_pixel(p, false, true);
            } else if !on_canvas(pos) {
                app.deselect();
            } else if !app.point_in_selection(p) {
                // A plain tap on a pixel selects just that pixel (1x1).
                app.select_pixel(p, false, false);
            }
        }
    }
    if response.secondary_clicked() {
        app.deselect();
    }
}

fn handle_magic_wand(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);
    if response.drag_started() && ui.input(|i| i.pointer.primary_down()) {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if on_canvas(pos) && app.point_in_selection(to_canvas(pos)) {
                app.begin_selection_drag(to_canvas(pos), Tool::Select, false, false);
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.update_selection_drag(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.end_selection_drag();
    }
    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                let p = to_canvas(pos);
                if shift {
                    app.magic_wand_select(p, true, false);
                } else if alt {
                    app.magic_wand_select(p, false, true);
                } else if !app.point_in_selection(p) {
                    app.magic_wand_select(p, false, false);
                }
            }
        }
    }
    if response.secondary_clicked() {
        app.deselect();
    }
}

fn handle_selection_brush(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);

    if response.drag_started() {
        let (primary, secondary) =
            ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        // Ignore the middle button (used for panning).
        if primary || secondary {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos());
            if let Some(pos) = origin {
                if on_canvas(pos) {
                    let p = to_canvas(pos);
                    // Plain left press inside the selection moves it; otherwise paint.
                    if !secondary && !alt && !shift && app.point_in_selection(p) {
                        app.begin_selection_drag(p, Tool::Select, false, false);
                    } else {
                        app.selection_brush_begin(p, secondary || alt, shift);
                    }
                }
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let p = to_canvas(pos);
            app.update_selection_drag(p);
            app.selection_brush_to(p);
        }
    }
    if response.drag_stopped() {
        app.end_selection_drag();
        app.selection_brush_end();
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                let p = to_canvas(pos);
                // A plain tap inside the selection does nothing (move territory).
                if shift || alt || !app.point_in_selection(p) {
                    app.selection_brush_begin(p, alt, shift);
                    app.selection_brush_end();
                }
            }
        }
    }
    if response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.selection_brush_begin(to_canvas(pos), true, false);
                app.selection_brush_end();
            }
        }
    }
}

/// Draws the animated marching-ants outline of the active selection.
fn paint_selection_overlay(
    app: &PaintApp,
    ui: &egui::Ui,
    canvas_rect: egui::Rect,
    magnification: f32,
) {
    let phase = (ui.input(|i| i.time) as f32 * 24.0) % (ANTS_DASH * 2.0);
    let painter = ui.painter();
    let mut drew = false;

    // Translucent highlight over the selected area.
    if let Some(runs) = app.selection_brush_runs() {
        paint_selection_highlight(
            painter,
            ui.clip_rect(),
            canvas_rect,
            magnification,
            runs,
            (0, 0),
        );
    } else if let Some((runs, origin)) = app.selection_runs() {
        paint_selection_highlight(
            painter,
            ui.clip_rect(),
            canvas_rect,
            magnification,
            runs,
            origin,
        );
    }

    if let Some(segments) = app.selection_brush_boundary() {
        // While painting a Selection Brush stroke, show only its outline
        // (static); the existing selection's animated outline is suppressed so
        // no stale edges remain at the intersections.
        paint_mask_boundary(painter, canvas_rect, magnification, segments, (0, 0), 0.0);
    } else {
        // The active selection (mask boundary or rect/lasso outline)...
        if let Some((segments, origin)) = app.selection_boundary() {
            paint_mask_boundary(painter, canvas_rect, magnification, segments, origin, phase);
            drew = true;
        }
        // ...plus the in-progress selection (visible while accumulating).
        for outline in [app.selection_outline(), app.creating_outline()]
            .into_iter()
            .flatten()
        {
            let poly = outline_to_poly(outline, canvas_rect, magnification);
            if poly.len() >= 2 {
                paint_ants(painter, &poly, phase);
                drew = true;
            }
        }
    }

    if drew {
        ui.ctx().request_repaint();
    }
}

/// Converts a selection outline into screen-space points.
fn outline_to_poly(
    outline: SelectionOutline,
    canvas_rect: egui::Rect,
    magnification: f32,
) -> Vec<Pos2> {
    let to_screen = |(x, y): (i32, i32)| -> Pos2 {
        canvas_rect.min + egui::vec2(x as f32 * magnification, y as f32 * magnification)
    };
    match outline {
        SelectionOutline::Rect {
            x,
            y,
            width,
            height,
        } => {
            let min = to_screen((x, y));
            let max = to_screen((x + width as i32, y + height as i32));
            vec![
                egui::pos2(min.x, min.y),
                egui::pos2(max.x, min.y),
                egui::pos2(max.x, max.y),
                egui::pos2(min.x, max.y),
            ]
        }
        SelectionOutline::Poly(points) => points.iter().map(|&p| to_screen(p)).collect(),
    }
}

/// Draws the boundary of a pixel-mask (magic wand) selection as marching ants,
/// using the precomputed outline segments (same style as the free-form select).
fn paint_mask_boundary(
    painter: &egui::Painter,
    canvas_rect: egui::Rect,
    magnification: f32,
    segments: &[rspaint_core::selection::BoundarySegment],
    (ox, oy): (i32, i32),
    phase: f32,
) {
    let to_screen = |(x, y): (i32, i32)| -> Pos2 {
        canvas_rect.min
            + egui::vec2(
                (ox + x) as f32 * magnification,
                (oy + y) as f32 * magnification,
            )
    };
    for &(start, end) in segments {
        paint_ant_segment(painter, to_screen(start), to_screen(end), phase);
    }
}

/// Draws a translucent white highlight over the selected area (horizontal runs).
fn paint_selection_highlight(
    painter: &egui::Painter,
    clip: egui::Rect,
    canvas_rect: egui::Rect,
    magnification: f32,
    runs: &[(i32, i32, i32)],
    (ox, oy): (i32, i32),
) {
    let visible = canvas_rect.intersect(clip);
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return;
    }
    let fill = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 60);
    for &(y, x0, x1) in runs {
        let min = canvas_rect.min
            + egui::vec2(
                (ox + x0) as f32 * magnification,
                (oy + y) as f32 * magnification,
            );
        let max = min + egui::vec2((x1 - x0 + 1) as f32 * magnification, magnification);
        let rect = egui::Rect::from_min_max(min, max).intersect(visible);
        if rect.width() > 0.0 && rect.height() > 0.0 {
            painter.rect_filled(rect, 0.0, fill);
        }
    }
}

const ANTS_DASH: f32 = 4.0;

/// Draws one marching-ants segment: alternating black/white dashes offset by
/// `phase`.
fn paint_ant_segment(painter: &egui::Painter, a: Pos2, b: Pos2, phase: f32) {
    let black = egui::Stroke::new(1.0, egui::Color32::BLACK);
    let white = egui::Stroke::new(1.0, egui::Color32::WHITE);
    let delta = b - a;
    let length = delta.length();
    if length <= 0.0 {
        return;
    }
    let dir = delta / length;
    let mut t = -phase;
    while t < length {
        let black_end = (t + ANTS_DASH).min(length);
        if black_end > t.max(0.0) {
            painter.line_segment([a + dir * t.max(0.0), a + dir * black_end], black);
        }
        let white_start = (t + ANTS_DASH).max(0.0);
        let white_end = (t + ANTS_DASH * 2.0).min(length);
        if white_end > white_start {
            painter.line_segment([a + dir * white_start, a + dir * white_end], white);
        }
        t += ANTS_DASH * 2.0;
    }
}

fn paint_ants(painter: &egui::Painter, poly: &[Pos2], phase: f32) {
    let count = poly.len();
    for i in 0..count {
        paint_ant_segment(painter, poly[i], poly[(i + 1) % count], phase);
    }
}

fn handle_navigation(
    app: &mut PaintApp,
    ui: &egui::Ui,
    viewport_rect: egui::Rect,
    viewport_size: Vec2,
) {
    let (pointer, delta, middle, scroll, zoom, shift) = ui.input(|i| {
        (
            i.pointer.interact_pos(),
            i.pointer.delta(),
            i.pointer.middle_down(),
            i.smooth_scroll_delta,
            // Ctrl/Cmd + wheel is delivered by egui as a zoom factor, not scroll.
            i.zoom_delta(),
            i.modifiers.shift,
        )
    });
    let over_viewport = pointer.is_some_and(|p| viewport_rect.contains(p));

    if middle && over_viewport {
        app.pan += delta;
    }

    if zoom != 1.0 && over_viewport {
        let anchor = pointer.unwrap_or_else(|| viewport_rect.center());
        let old_mag = app.magnification;
        let new_mag = (old_mag * zoom).clamp(MIN_MAGNIFICATION, MAX_MAGNIFICATION);
        if new_mag != old_mag {
            // Keep the canvas point under the pointer fixed.
            let canvas_min = viewport_rect.min + app.pan;
            let anchored = (anchor - canvas_min) / old_mag;
            app.pan = anchor - viewport_rect.min - anchored * new_mag;
            app.magnification = new_mag;
        }
    }

    if scroll != Vec2::ZERO && over_viewport {
        if shift {
            app.pan.x += scroll.y;
        } else {
            app.pan.y += scroll.y;
        }
    }

    clamp_pan(app, viewport_size);
}

/// Keeps the canvas covering the viewport, centering it when it is smaller.
fn clamp_pan(app: &mut PaintApp, viewport_size: Vec2) {
    let content = egui::vec2(
        app.document.width() as f32 * app.magnification,
        app.document.height() as f32 * app.magnification,
    );
    app.pan.x = clamp_axis(app.pan.x, content.x, viewport_size.x);
    app.pan.y = clamp_axis(app.pan.y, content.y, viewport_size.y);
}

fn clamp_axis(pan: f32, content: f32, viewport: f32) -> f32 {
    if content <= viewport {
        (viewport - content) / 2.0
    } else {
        pan.clamp(viewport - content, 0.0)
    }
}

fn handle_drawing(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let shift = ui.input(|i| i.modifiers.shift);
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);

    if response.drag_started() {
        let (primary, secondary) =
            ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        let button = if secondary {
            Some(PointerButton::Secondary)
        } else if primary {
            Some(PointerButton::Primary)
        } else {
            None
        };
        // Start at the point where the button was pressed, not the current
        // pointer position, which may already have moved away (fast strokes).
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let (Some(button), Some(pos)) = (button, origin) {
            if on_canvas(pos) {
                app.begin_stroke(to_canvas(pos), button);
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.update_stroke(to_canvas(pos), shift);
        }
    }
    if response.drag_stopped() {
        app.end_stroke(shift);
    }

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                let p = to_canvas(pos);
                if app.tool.is_click_tool() {
                    app.click_tool(p, PointerButton::Primary, shift);
                } else if app.tool.is_freehand() || app.tool.is_shape() {
                    app.begin_stroke(p, PointerButton::Primary);
                    app.end_stroke(shift);
                }
            }
        }
    }
    if response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                let p = to_canvas(pos);
                if app.tool.is_click_tool() {
                    app.click_tool(p, PointerButton::Secondary, shift);
                } else if app.tool.is_freehand() || app.tool.is_shape() {
                    app.begin_stroke(p, PointerButton::Secondary);
                    app.end_stroke(shift);
                }
            }
        }
    }
}

/// The visible sub-rectangle of the canvas as `[min_x, min_y, size_x, size_y]`
/// in `0..1` uv space.
fn visible_uv_rect(canvas: egui::Rect, visible: egui::Rect) -> [f32; 4] {
    let size = canvas.size();
    [
        (visible.min.x - canvas.min.x) / size.x,
        (visible.min.y - canvas.min.y) / size.y,
        visible.width() / size.x,
        visible.height() / size.y,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_canvas_maps_to_unit_uv() {
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        assert_eq!(visible_uv_rect(canvas, canvas), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn sub_rect_maps_to_uv_range() {
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let visible = egui::Rect::from_min_size(egui::pos2(25.0, 10.0), egui::vec2(50.0, 25.0));
        assert_eq!(visible_uv_rect(canvas, visible), [0.25, 0.2, 0.5, 0.5]);
    }

    #[test]
    fn offset_canvas_maps_correctly() {
        let canvas = egui::Rect::from_min_size(egui::pos2(-200.0, 0.0), egui::vec2(400.0, 100.0));
        let visible = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
        assert_eq!(visible_uv_rect(canvas, visible), [0.5, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn small_content_is_centered() {
        assert_eq!(clamp_axis(123.0, 100.0, 300.0), 100.0);
    }

    #[test]
    fn large_content_is_clamped_within_bounds() {
        assert_eq!(clamp_axis(50.0, 1000.0, 300.0), 0.0);
        assert_eq!(clamp_axis(-5000.0, 1000.0, 300.0), -700.0);
    }
}

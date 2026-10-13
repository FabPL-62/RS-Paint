use super::*;

/// Draws the light-blue border and the resize handle squares.
pub(super) fn paint_resize_handles(
    painter: &egui::Painter,
    canvas_rect: egui::Rect,
    hovered: Option<ResizeHandle>,
) {
    let blue = egui::Color32::from_rgb(120, 180, 240);
    let active = egui::Color32::from_rgb(70, 140, 230);
    painter.rect_stroke(
        canvas_rect,
        0.0,
        egui::Stroke::new(1.5, blue),
        egui::StrokeKind::Outside,
    );
    for handle in ResizeHandle::ALL {
        let rect = resize_handle_rect(canvas_rect, handle);
        let color = if Some(handle) == hovered {
            active
        } else {
            blue
        };
        painter.rect_filled(rect, 0.0, color);
    }
}

/// Draws the selection transform box and its resize handles.
pub(super) fn paint_selection_box(
    app: &PaintApp,
    ui: &egui::Ui,
    box_rect: egui::Rect,
    hovered: Option<ResizeHandle>,
) {
    let painter = ui.painter();
    if app.editor.selection_is_simple_rect() {
        // Plain rectangle: white handles with black borders.
        for handle in ResizeHandle::ALL {
            let rect = box_handle_rect(box_rect, handle);
            let fill = if Some(handle) == hovered {
                egui::Color32::from_gray(170)
            } else {
                egui::Color32::WHITE
            };
            painter.rect_filled(rect, 0.0, fill);
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, egui::Color32::BLACK),
                egui::StrokeKind::Inside,
            );
        }
    } else {
        // Free-form / combined: a marching-ants box with cyan handles. The box
        // is ants-only (matching the selection outline) so that where the two
        // coincide they read as a single line.
        let poly = [
            box_rect.min,
            egui::pos2(box_rect.max.x, box_rect.min.y),
            box_rect.max,
            egui::pos2(box_rect.min.x, box_rect.max.y),
        ];
        let phase = (ui.input(|i| i.time) as f32 * 24.0) % (ANTS_DASH * 2.0);
        paint_ants(painter, &poly, phase);
        for handle in ResizeHandle::ALL {
            let rect = box_handle_rect(box_rect, handle);
            let fill = if Some(handle) == hovered {
                egui::Color32::from_rgb(0, 130, 130)
            } else {
                egui::Color32::from_rgb(0, 210, 210)
            };
            painter.rect_filled(rect, 0.0, fill);
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, egui::Color32::BLACK),
                egui::StrokeKind::Inside,
            );
        }
    }
    ui.ctx().request_repaint();
}

/// Draws the rotation pivot cross, the line and the handle circle.
pub(super) fn paint_selection_rotation(
    app: &PaintApp,
    ui: &egui::Ui,
    pivot: egui::Pos2,
    handle: egui::Pos2,
    pivot_hovered: bool,
    circle_hovered: bool,
) {
    let painter = ui.painter();
    let black = egui::Stroke::new(1.0, egui::Color32::BLACK);
    painter.line_segment([pivot, handle], black);
    let fill = if circle_hovered {
        egui::Color32::from_gray(150)
    } else {
        egui::Color32::WHITE
    };
    painter.circle_filled(handle, CIRCLE_RADIUS, fill);
    painter.circle_stroke(handle, CIRCLE_RADIUS, black);
    let opacity = if pivot_hovered || app.editor.pivot_dragging() || app.editor.rotating_selection()
    {
        1.0
    } else {
        0.5
    };
    app.paint_pivot_cross(painter, pivot, opacity);
}

/// Draws the in-progress magnifier zoom rectangle (marching ants).
pub(super) fn paint_zoom_rect(
    app: &PaintApp,
    ui: &egui::Ui,
    canvas_rect: egui::Rect,
    magnification: f32,
) {
    let Some((a, b)) = app.editor.zoom_rect() else {
        return;
    };
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
    let poly = [min, egui::pos2(max.x, min.y), max, egui::pos2(min.x, max.y)];
    let phase = (ui.input(|i| i.time) as f32 * 24.0) % (ANTS_DASH * 2.0);
    paint_ants(ui.painter(), &poly, phase);
    ui.ctx().request_repaint();
}

/// Draws the text box being created and hosts the editor for the active box.
pub(super) fn paint_text_overlay(
    app: &mut PaintApp,
    ui: &mut egui::Ui,
    canvas_rect: egui::Rect,
    magnification: f32,
) {
    if let Some((a, b)) = app.editor.text_creating_rect() {
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

    let Some((x, y, width, height)) = app.editor.text_box_rect() else {
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
        if let Some((a, b)) = app.editor.text_caret_segment() {
            let p0 = min + egui::vec2(a.0 * magnification, a.1 * magnification);
            let p1 = min + egui::vec2(b.0 * magnification, b.1 * magnification);
            ui.painter().line_segment(
                [p0, p1],
                egui::Stroke::new(1.0, to_color32(app.editor.settings.foreground)),
            );
        }
    }
    ui.ctx().request_repaint();
}

/// Draws the in-progress polygon outline and the rubber-band preview.
pub(super) fn paint_polygon_overlay(
    app: &PaintApp,
    ui: &egui::Ui,
    canvas_rect: egui::Rect,
    magnification: f32,
) {
    if app.editor.polygon_points().is_empty() {
        return;
    }
    let to_screen = |(x, y): (i32, i32)| -> Pos2 {
        canvas_rect.min
            + egui::vec2(
                (x as f32 + 0.5) * magnification,
                (y as f32 + 0.5) * magnification,
            )
    };
    let mut poly: Vec<Pos2> = app
        .editor
        .polygon_points()
        .iter()
        .map(|&p| to_screen(p))
        .collect();
    if let Some(preview) = app.editor.polygon_preview() {
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

/// Draws the animated marching-ants outline of the active selection.
pub(super) fn paint_selection_overlay(
    app: &PaintApp,
    ui: &egui::Ui,
    canvas_rect: egui::Rect,
    magnification: f32,
    show_highlight: bool,
) {
    let phase = (ui.input(|i| i.time) as f32 * 24.0) % (ANTS_DASH * 2.0);
    let painter = ui.painter();
    let mut drew = false;

    // Translucent highlight over the selected area (hidden while drawing is
    // clipped to the selection, so the real colors show through).
    if show_highlight {
        if let Some(runs) = app.editor.selection_brush_runs() {
            paint_selection_highlight(
                painter,
                ui.clip_rect(),
                canvas_rect,
                magnification,
                runs,
                (0, 0),
            );
        } else if let Some((runs, origin)) = app.editor.selection_runs() {
            paint_selection_highlight(
                painter,
                ui.clip_rect(),
                canvas_rect,
                magnification,
                runs,
                origin,
            );
        }
    }

    if let Some(segments) = app.editor.selection_brush_boundary() {
        // While painting a Selection Brush stroke, show only its outline
        // (static); the existing selection's animated outline is suppressed so
        // no stale edges remain at the intersections.
        paint_mask_boundary(painter, canvas_rect, magnification, segments, (0, 0), 0.0);
    } else {
        // The active selection (mask boundary or rect/lasso outline)...
        if let Some((segments, origin)) = app.editor.selection_boundary() {
            paint_mask_boundary(painter, canvas_rect, magnification, segments, origin, phase);
            drew = true;
        }
        // ...plus the in-progress selection (visible while accumulating).
        for outline in [
            app.editor.selection_outline(),
            app.editor.creating_outline(),
        ]
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
pub(super) fn outline_to_poly(
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
pub(super) fn paint_mask_boundary(
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
pub(super) fn paint_selection_highlight(
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

/// Draws one marching-ants segment: alternating black/white dashes offset by
/// `phase`.
///
/// The dash pattern is anchored to the screen coordinate along the segment's
/// dominant axis (not to the segment's own start), so segments that lie on the
/// same line -- e.g. the transform box edge and the selection outline it sits
/// on -- render as a single set of ants instead of two offset sets.
pub(super) fn paint_ant_segment(painter: &egui::Painter, a: Pos2, b: Pos2, phase: f32) {
    let delta = b - a;
    if delta.length() <= 0.0 {
        return;
    }
    let black = egui::Stroke::new(1.0, egui::Color32::BLACK);
    let white = egui::Stroke::new(1.0, egui::Color32::WHITE);
    let horizontal = delta.x.abs() >= delta.y.abs();
    let (lo, hi, fixed) = if horizontal {
        (a.x.min(b.x), a.x.max(b.x), a.y)
    } else {
        (a.y.min(b.y), a.y.max(b.y), a.x)
    };
    let dash = ANTS_DASH;
    let first = ((lo - phase) / dash).floor() as i32;
    let last = ((hi - phase) / dash).ceil() as i32;
    for k in first..last {
        let cell_start = (k as f32) * dash + phase;
        let start = cell_start.max(lo);
        let end = (cell_start + dash).min(hi);
        if end <= start {
            continue;
        }
        let stroke = if k % 2 == 0 { black } else { white };
        let (p0, p1) = if horizontal {
            (egui::pos2(start, fixed), egui::pos2(end, fixed))
        } else {
            (egui::pos2(fixed, start), egui::pos2(fixed, end))
        };
        painter.line_segment([p0, p1], stroke);
    }
}

pub(super) fn paint_ants(painter: &egui::Painter, poly: &[Pos2], phase: f32) {
    let count = poly.len();
    for i in 0..count {
        paint_ant_segment(painter, poly[i], poly[(i + 1) % count], phase);
    }
}

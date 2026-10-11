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
    to_color32, PaintApp, PickTarget, ResizeHandle, SelectionOutline, MAX_MAGNIFICATION,
    MIN_MAGNIFICATION,
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
    app.set_viewport_size(viewport_size);
    let (viewport_rect, response) =
        ui.allocate_exact_size(viewport_size, egui::Sense::click_and_drag());

    handle_navigation(app, ui, viewport_rect, viewport_size);

    let magnification = app.magnification;
    let doc_size = egui::vec2(
        app.document.width() as f32 * magnification,
        app.document.height() as f32 * magnification,
    );
    let canvas_rect = egui::Rect::from_min_size(viewport_rect.min + app.pan, doc_size);

    // Over the visible canvas (the paper, clipped to the viewport) hide the OS
    // pointer and draw the tool's own cursor image instead. Over the gray area
    // outside the paper the system pointer stays visible. Near a resize handle
    // the system cursor shows the matching resize icon.
    let visible_canvas = canvas_rect.intersect(viewport_rect);
    let cursor_pos = ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()));
    // When a popup (e.g. the color picker) is above the canvas, keep the system
    // pointer visible instead of hiding it behind the popup.
    let occluded = cursor_pos
        .and_then(|pos| ui.ctx().layer_id_at(pos))
        .is_some_and(|layer| layer != ui.layer_id());
    let hovered_handle = if occluded {
        None
    } else {
        cursor_pos.and_then(|pos| hovered_resize_handle(canvas_rect, pos))
    };
    // While drawing is clipped to the selection, hide the transform box and
    // rotation gizmo (only the marching ants remain).
    let clipping = app.clipping_to_selection();
    let selection_box = if clipping {
        None
    } else {
        app.selection_bounds().map(|(x, y, w, h)| {
            egui::Rect::from_min_size(
                canvas_rect.min + egui::vec2(x as f32 * magnification, y as f32 * magnification),
                egui::vec2(w as f32 * magnification, h as f32 * magnification),
            )
        })
    };
    let selection_handle = if clipping || occluded {
        None
    } else {
        cursor_pos.and_then(|pos| selection_box.and_then(|rect| hovered_box_handle(rect, pos)))
    };
    // The rotation pivot (cross) and its handle (line + circle).
    let pivot_screen = if clipping {
        None
    } else {
        app.selection_pivot().map(|pivot| {
            canvas_rect.min + egui::vec2(pivot.0 * magnification, pivot.1 * magnification)
        })
    };
    let rotation_handle = pivot_screen.map(|pivot| rotation_handle_pos(app, pivot));
    let pivot_hovered = cursor_pos
        .zip(pivot_screen)
        .is_some_and(|(cursor, pivot)| (cursor - pivot).length() <= CROSS_HIT);
    let circle_hovered = cursor_pos
        .zip(rotation_handle)
        .is_some_and(|(cursor, handle)| (cursor - handle).length() <= CIRCLE_HIT);
    let over_canvas = !occluded && cursor_pos.is_some_and(|pos| visible_canvas.contains(pos));
    if let Some(handle) = hovered_handle {
        ui.ctx().set_cursor_icon(handle_cursor(handle));
    } else if let Some(handle) = selection_handle {
        ui.ctx().set_cursor_icon(handle_cursor(handle));
    } else if pivot_hovered || circle_hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    } else if over_canvas {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    }
    let custom_cursor = over_canvas
        && hovered_handle.is_none()
        && selection_handle.is_none()
        && !pivot_hovered
        && !circle_hovered;

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

    let resizing = handle_canvas_resize(
        app,
        ui,
        &response,
        canvas_rect,
        viewport_rect,
        magnification,
    );
    let resizing_selection =
        !resizing && handle_selection_resize(app, ui, &response, canvas_rect, magnification);
    let rotating = !resizing
        && !resizing_selection
        && handle_selection_rotation(app, ui, &response, canvas_rect, magnification);
    if resizing || resizing_selection || rotating {
        // The transform gizmo owns the input this frame.
    } else if let Some(target) = app.pick_hold() {
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
    } else if app.tool == Tool::Magnifier {
        handle_magnifier(app, ui, &response, canvas_rect, viewport_rect, &to_canvas);
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

    paint_selection_overlay(app, ui, canvas_rect, magnification, !clipping);
    if let Some(rect) = selection_box {
        paint_selection_box(app, ui, rect, selection_handle);
    }
    if let (Some(pivot), Some(handle)) = (pivot_screen, rotation_handle) {
        paint_selection_rotation(app, ui, pivot, handle, pivot_hovered, circle_hovered);
    }
    paint_zoom_rect(app, ui, canvas_rect, magnification);
    paint_polygon_overlay(app, ui, canvas_rect, magnification);
    paint_text_overlay(app, ui, canvas_rect, magnification);

    // Resize handles appear near the border, while hovering one, or resizing.
    let near_border = cursor_pos.is_some_and(|pos| {
        canvas_rect.expand(HANDLE_MARGIN).contains(pos)
            && !canvas_rect.shrink(HANDLE_MARGIN).contains(pos)
    });
    if hovered_handle.is_some() || app.resizing_canvas() || near_border {
        paint_resize_handles(ui.painter(), canvas_rect, hovered_handle);
    }

    // Draw the custom tool cursor (the OS pointer is hidden over the canvas).
    if custom_cursor {
        if let Some(pos) = cursor_pos {
            app.paint_cursor(ui.painter(), pos);
        }
    }
}

/// The size of a resize handle square, in points.
const HANDLE_SIZE: f32 = 8.0;
/// Extra hit area around a handle.
const HANDLE_HIT: f32 = 4.0;
/// How close to the border the handles appear.
const HANDLE_MARGIN: f32 = 12.0;

/// The screen rectangle of a resize handle (just outside the canvas border).
fn resize_handle_rect(canvas_rect: egui::Rect, handle: ResizeHandle) -> egui::Rect {
    let s = HANDLE_SIZE;
    let cx = canvas_rect.center().x;
    let cy = canvas_rect.center().y;
    let (min_x, max_x, min_y, max_y) = match handle {
        ResizeHandle::North => (
            cx - s / 2.0,
            cx + s / 2.0,
            canvas_rect.min.y - s,
            canvas_rect.min.y,
        ),
        ResizeHandle::South => (
            cx - s / 2.0,
            cx + s / 2.0,
            canvas_rect.max.y,
            canvas_rect.max.y + s,
        ),
        ResizeHandle::East => (
            canvas_rect.max.x,
            canvas_rect.max.x + s,
            cy - s / 2.0,
            cy + s / 2.0,
        ),
        ResizeHandle::West => (
            canvas_rect.min.x - s,
            canvas_rect.min.x,
            cy - s / 2.0,
            cy + s / 2.0,
        ),
        ResizeHandle::NorthWest => (
            canvas_rect.min.x - s,
            canvas_rect.min.x,
            canvas_rect.min.y - s,
            canvas_rect.min.y,
        ),
        ResizeHandle::NorthEast => (
            canvas_rect.max.x,
            canvas_rect.max.x + s,
            canvas_rect.min.y - s,
            canvas_rect.min.y,
        ),
        ResizeHandle::SouthWest => (
            canvas_rect.min.x - s,
            canvas_rect.min.x,
            canvas_rect.max.y,
            canvas_rect.max.y + s,
        ),
        ResizeHandle::SouthEast => (
            canvas_rect.max.x,
            canvas_rect.max.x + s,
            canvas_rect.max.y,
            canvas_rect.max.y + s,
        ),
    };
    egui::Rect::from_min_max(egui::pos2(min_x, min_y), egui::pos2(max_x, max_y))
}

/// The handle under `pointer`, if any.
fn hovered_resize_handle(canvas_rect: egui::Rect, pointer: egui::Pos2) -> Option<ResizeHandle> {
    ResizeHandle::ALL.into_iter().find(|&handle| {
        resize_handle_rect(canvas_rect, handle)
            .expand(HANDLE_HIT)
            .contains(pointer)
    })
}

/// The resize cursor for a handle.
fn handle_cursor(handle: ResizeHandle) -> egui::CursorIcon {
    match handle {
        ResizeHandle::East | ResizeHandle::West => egui::CursorIcon::ResizeHorizontal,
        ResizeHandle::North | ResizeHandle::South => egui::CursorIcon::ResizeVertical,
        ResizeHandle::NorthWest | ResizeHandle::SouthEast => egui::CursorIcon::ResizeNwSe,
        ResizeHandle::NorthEast | ResizeHandle::SouthWest => egui::CursorIcon::ResizeNeSw,
    }
}

/// Draws the light-blue border and the resize handle squares.
fn paint_resize_handles(
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

/// The screen rectangle of a selection transform-box handle (centered on the box).
fn box_handle_rect(box_rect: egui::Rect, handle: ResizeHandle) -> egui::Rect {
    let s = HANDLE_SIZE;
    let cx = box_rect.center().x;
    let cy = box_rect.center().y;
    let center = match handle {
        ResizeHandle::North => egui::pos2(cx, box_rect.min.y),
        ResizeHandle::South => egui::pos2(cx, box_rect.max.y),
        ResizeHandle::East => egui::pos2(box_rect.max.x, cy),
        ResizeHandle::West => egui::pos2(box_rect.min.x, cy),
        ResizeHandle::NorthWest => egui::pos2(box_rect.min.x, box_rect.min.y),
        ResizeHandle::NorthEast => egui::pos2(box_rect.max.x, box_rect.min.y),
        ResizeHandle::SouthWest => egui::pos2(box_rect.min.x, box_rect.max.y),
        ResizeHandle::SouthEast => egui::pos2(box_rect.max.x, box_rect.max.y),
    };
    egui::Rect::from_center_size(center, egui::vec2(s, s))
}

/// The selection transform-box handle under `pointer`, if any.
fn hovered_box_handle(box_rect: egui::Rect, pointer: egui::Pos2) -> Option<ResizeHandle> {
    ResizeHandle::ALL.into_iter().find(|&handle| {
        box_handle_rect(box_rect, handle)
            .expand(HANDLE_HIT)
            .contains(pointer)
    })
}

/// The selection box handle and its top-left under `pointer`, if any.
fn selection_handle_at(
    app: &PaintApp,
    canvas_rect: egui::Rect,
    pointer: egui::Pos2,
) -> Option<(ResizeHandle, egui::Pos2)> {
    let (x, y, w, h) = app.selection_bounds()?;
    let mag = app.magnification;
    let min = canvas_rect.min + egui::vec2(x as f32 * mag, y as f32 * mag);
    let rect = egui::Rect::from_min_size(min, egui::vec2(w as f32 * mag, h as f32 * mag));
    hovered_box_handle(rect, pointer).map(|handle| (handle, min))
}

/// Draws the selection transform box and its resize handles.
fn paint_selection_box(
    app: &PaintApp,
    ui: &egui::Ui,
    box_rect: egui::Rect,
    hovered: Option<ResizeHandle>,
) {
    let painter = ui.painter();
    if app.selection_is_simple_rect() {
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
        // Free-form / combined: dark-blue marching-ants box with cyan handles.
        let dark_blue = egui::Color32::from_rgb(20, 40, 150);
        painter.rect_stroke(
            box_rect,
            0.0,
            egui::Stroke::new(1.0, dark_blue),
            egui::StrokeKind::Outside,
        );
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

/// The distance from the pivot to the rotation handle, in screen points.
const HANDLE_DIST: f32 = 40.0;
/// The rotation handle circle radius.
const CIRCLE_RADIUS: f32 = 5.0;
/// Hit radius for the pivot cross.
const CROSS_HIT: f32 = 8.0;
/// Hit radius for the rotation handle circle.
const CIRCLE_HIT: f32 = 8.0;

/// The screen position of the rotation handle for a pivot.
fn rotation_handle_pos(app: &PaintApp, pivot: egui::Pos2) -> egui::Pos2 {
    let angle = app.selection_rotate_angle();
    let direction = egui::vec2(angle.sin(), -angle.cos()) * HANDLE_DIST;
    pivot + direction
}

/// Handles dragging the rotation pivot and the rotation handle.
fn handle_selection_rotation(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    magnification: f32,
) -> bool {
    let Some(pivot) = app.selection_pivot() else {
        return false;
    };
    let pivot_screen =
        canvas_rect.min + egui::vec2(pivot.0 * magnification, pivot.1 * magnification);
    let handle_screen = rotation_handle_pos(app, pivot_screen);
    let to_image = |pos: egui::Pos2| -> (f32, f32) {
        (
            (pos.x - canvas_rect.min.x) / magnification,
            (pos.y - canvas_rect.min.y) / magnification,
        )
    };

    let shift = ui.input(|i| i.modifiers.shift);
    if app.rotating_selection() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.update_selection_rotate(to_image(pos), shift);
        }
        if response.drag_stopped() {
            app.finish_selection_rotate();
        }
        return true;
    }
    if app.pivot_dragging() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.move_selection_pivot(to_image(pos));
        }
        if response.drag_stopped() {
            app.set_pivot_dragging(false);
        }
        return true;
    }
    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if (pos - handle_screen).length() <= CIRCLE_HIT {
                app.begin_selection_rotate(to_image(pos));
                app.update_selection_rotate(to_image(pos), shift);
                return true;
            }
            if (pos - pivot_screen).length() <= CROSS_HIT {
                app.begin_pivot_drag(to_image(pos));
                return true;
            }
        }
    }
    false
}

/// Draws the rotation pivot cross, the line and the handle circle.
fn paint_selection_rotation(
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
    let opacity = if pivot_hovered || app.pivot_dragging() || app.rotating_selection() {
        1.0
    } else {
        0.5
    };
    app.paint_pivot_cross(painter, pivot, opacity);
}

/// Handles a canvas resize drag; returns whether it consumed the input.
fn handle_canvas_resize(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    viewport_rect: egui::Rect,
    magnification: f32,
) -> bool {
    if app.resizing_canvas() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.update_canvas_resize(pos, viewport_rect.min);
        }
        if response.drag_stopped() {
            app.finish_canvas_resize();
        }
        return true;
    }
    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(handle) = origin.and_then(|pos| hovered_resize_handle(canvas_rect, pos)) {
            app.begin_canvas_resize(handle, canvas_rect.min, magnification);
            if let Some(pos) = origin {
                app.update_canvas_resize(pos, viewport_rect.min);
            }
            return true;
        }
    }
    false
}

/// Handles a selection-content resize drag; returns whether it consumed input.
fn handle_selection_resize(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    magnification: f32,
) -> bool {
    if app.resizing_selection() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.update_selection_resize(pos);
        }
        if response.drag_stopped() {
            app.finish_selection_resize();
        }
        return true;
    }
    if response.drag_started() && ui.input(|i| i.pointer.primary_down()) {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if let Some((handle, start_min)) = selection_handle_at(app, canvas_rect, pos) {
                app.begin_selection_resize(handle, start_min, magnification);
                app.update_selection_resize(pos);
                return true;
            }
        }
    }
    false
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

/// Handles the Magnifier: drag a rectangle to zoom into it, click to zoom.
fn handle_magnifier(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    viewport_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);
    if response.drag_started() && ui.input(|i| i.pointer.primary_down()) {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.begin_zoom_rect(to_canvas(pos));
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.update_zoom_rect(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.apply_zoom_rect(viewport_rect.size());
    }
    if response.clicked() {
        app.zoom_in();
    }
    if response.secondary_clicked() {
        app.zoom_out();
    }
}

/// Draws the in-progress magnifier zoom rectangle (marching ants).
fn paint_zoom_rect(app: &PaintApp, ui: &egui::Ui, canvas_rect: egui::Rect, magnification: f32) {
    let Some((a, b)) = app.zoom_rect() else {
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

    if response.drag_started() {
        let (primary, secondary) =
            ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        // Contour Select uses right-click to drop a contour, not to subtract.
        let right_subtract = secondary && tool != Tool::ContourSelect;
        if primary || right_subtract {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos());
            if let Some(pos) = origin {
                if on_canvas(pos) {
                    // Right-drag subtracts; Shift adds, Alt subtracts.
                    app.begin_selection_drag(
                        to_canvas(pos),
                        tool,
                        shift && !secondary,
                        alt || right_subtract,
                    );
                }
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
        match response.interact_pointer_pos() {
            // Contour Select drops the connected contour under the cursor.
            Some(pos) if on_canvas(pos) && tool == Tool::ContourSelect => {
                app.contour_deselect_at(to_canvas(pos));
            }
            // Otherwise right-click removes a pixel (anti-selection).
            Some(pos) if on_canvas(pos) => app.select_pixel(to_canvas(pos), false, true),
            _ => app.deselect(),
        }
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
        // Right-click subtracts (anti-selection) from the magic-wand selection.
        match response.interact_pointer_pos() {
            Some(pos) if on_canvas(pos) => app.magic_wand_select(to_canvas(pos), false, true),
            _ => app.deselect(),
        }
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
    show_highlight: bool,
) {
    let phase = (ui.input(|i| i.time) as f32 * 24.0) % (ANTS_DASH * 2.0);
    let painter = ui.painter();
    let mut drew = false;

    // Translucent highlight over the selected area (hidden while drawing is
    // clipped to the selection, so the real colors show through).
    if show_highlight {
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

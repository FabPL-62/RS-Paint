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

use crate::app::pointer_button;
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
        ui.label(crate::i18n::tr(
            ui.ctx(),
            "The GPU renderer is unavailable.",
        ));
        return;
    }
    // The full-screen bitmap view replaces the canvas (drawn as an overlay).
    if app.show_bitmap {
        ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
        return;
    }

    let viewport_size = ui.available_size();
    app.editor.set_viewport_size(viewport_size);
    let (viewport_rect, response) =
        ui.allocate_exact_size(viewport_size, egui::Sense::click_and_drag());

    handle_navigation(app, ui, viewport_rect, viewport_size);

    let magnification = app.editor.magnification;
    let doc_size = egui::vec2(
        app.editor.document.width() as f32 * magnification,
        app.editor.document.height() as f32 * magnification,
    );
    let canvas_rect = egui::Rect::from_min_size(viewport_rect.min + app.editor.pan, doc_size);

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
    // rotation gizmo (only the marching ants remain). The transform controls
    // are shown while Ctrl is held; without it a selection only shows its area.
    let clipping = app.editor.clipping_to_selection();
    let ctrl = ui.input(|i| i.modifiers.command);
    let show_transform = ctrl && !clipping && app.editor.settings.tool.is_selection();
    let selection_box = if !show_transform {
        None
    } else {
        app.editor.selection_bounds().map(|(x, y, w, h)| {
            egui::Rect::from_min_size(
                canvas_rect.min + egui::vec2(x as f32 * magnification, y as f32 * magnification),
                egui::vec2(w as f32 * magnification, h as f32 * magnification),
            )
        })
    };
    let selection_handle = if !show_transform || occluded {
        None
    } else {
        cursor_pos.and_then(|pos| selection_box.and_then(|rect| hovered_box_handle(rect, pos)))
    };
    // The rotation pivot (cross) and its handle (line + circle).
    let pivot_screen = if !show_transform {
        None
    } else {
        app.editor.selection_pivot().map(|pivot| {
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
            && p.0 < app.editor.document.width() as i32
            && p.1 < app.editor.document.height() as i32;
        inside.then_some(p)
    });
    app.editor.set_pointer(pointer);

    let resizing = handle_canvas_resize(
        app,
        ui,
        &response,
        canvas_rect,
        viewport_rect,
        magnification,
    );
    let resizing_selection = !resizing
        && (show_transform || app.editor.resizing_selection())
        && handle_selection_resize(app, ui, &response, canvas_rect, magnification);
    let rotating = !resizing
        && !resizing_selection
        && (show_transform || app.editor.rotating_selection() || app.editor.pivot_dragging())
        && handle_selection_rotation(app, ui, &response, canvas_rect, magnification);
    if resizing || resizing_selection || rotating {
        // The transform gizmo owns the input this frame.
    } else if let Some(target) = app.editor.pick_hold() {
        handle_pick(app, &response, canvas_rect, &to_canvas, target);
    } else if app.editor.settings.tool == Tool::MagicWand {
        handle_magic_wand(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.editor.settings.tool == Tool::SelectionBrush {
        handle_selection_brush(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.editor.settings.tool.is_selection() {
        handle_selection(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.editor.settings.tool == Tool::Polygon {
        handle_polygon(app, &response, canvas_rect, &to_canvas);
    } else if app.editor.settings.tool == Tool::Curve {
        handle_curve(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.editor.settings.tool == Tool::Text {
        handle_text(app, ui, &response, canvas_rect, &to_canvas);
    } else if app.editor.settings.tool == Tool::Magnifier {
        handle_magnifier(app, ui, &response, canvas_rect, viewport_rect, &to_canvas);
    } else {
        handle_drawing(app, ui, &response, canvas_rect, &to_canvas);
    }

    let visible = canvas_rect.intersect(viewport_rect);
    if visible.width() > 0.0 && visible.height() > 0.0 {
        let uv_rect = visible_uv_rect(canvas_rect, visible);
        let uniforms = app.canvas_uniforms(magnification, ui.ctx().pixels_per_point(), uv_rect);
        let upload = app.stage_canvas_upload();
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            visible,
            CanvasCallback {
                window_id: app.window_id(),
                uniforms,
                upload,
            },
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
    if hovered_handle.is_some() || app.editor.resizing_canvas() || near_border {
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

/// The distance from the pivot to the rotation handle, in screen points.
const HANDLE_DIST: f32 = 40.0;
/// The rotation handle circle radius.
const CIRCLE_RADIUS: f32 = 5.0;
/// Hit radius for the pivot cross.
const CROSS_HIT: f32 = 8.0;
/// Hit radius for the rotation handle circle.
const CIRCLE_HIT: f32 = 8.0;

const ANTS_DASH: f32 = 4.0;

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

mod geometry;
mod input;
mod overlays;
use geometry::*;
use input::*;
use overlays::*;

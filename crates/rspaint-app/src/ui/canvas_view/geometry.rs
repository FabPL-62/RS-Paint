use super::*;

/// The screen rectangle of a resize handle (just outside the canvas border).
pub(super) fn resize_handle_rect(canvas_rect: egui::Rect, handle: ResizeHandle) -> egui::Rect {
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
pub(super) fn hovered_resize_handle(
    canvas_rect: egui::Rect,
    pointer: egui::Pos2,
) -> Option<ResizeHandle> {
    ResizeHandle::ALL.into_iter().find(|&handle| {
        resize_handle_rect(canvas_rect, handle)
            .expand(HANDLE_HIT)
            .contains(pointer)
    })
}

/// The resize cursor for a handle.
pub(super) fn handle_cursor(handle: ResizeHandle) -> egui::CursorIcon {
    match handle {
        ResizeHandle::East | ResizeHandle::West => egui::CursorIcon::ResizeHorizontal,
        ResizeHandle::North | ResizeHandle::South => egui::CursorIcon::ResizeVertical,
        ResizeHandle::NorthWest | ResizeHandle::SouthEast => egui::CursorIcon::ResizeNwSe,
        ResizeHandle::NorthEast | ResizeHandle::SouthWest => egui::CursorIcon::ResizeNeSw,
    }
}

/// The screen rectangle of a selection transform-box handle (centered on the box).
pub(super) fn box_handle_rect(box_rect: egui::Rect, handle: ResizeHandle) -> egui::Rect {
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
pub(super) fn hovered_box_handle(
    box_rect: egui::Rect,
    pointer: egui::Pos2,
) -> Option<ResizeHandle> {
    ResizeHandle::ALL.into_iter().find(|&handle| {
        box_handle_rect(box_rect, handle)
            .expand(HANDLE_HIT)
            .contains(pointer)
    })
}

/// The selection box handle and its top-left under `pointer`, if any.
pub(super) fn selection_handle_at(
    app: &PaintApp,
    canvas_rect: egui::Rect,
    pointer: egui::Pos2,
) -> Option<(ResizeHandle, egui::Pos2)> {
    let (x, y, w, h) = app.editor.selection_bounds()?;
    let mag = app.editor.magnification;
    let min = canvas_rect.min + egui::vec2(x as f32 * mag, y as f32 * mag);
    let rect = egui::Rect::from_min_size(min, egui::vec2(w as f32 * mag, h as f32 * mag));
    hovered_box_handle(rect, pointer).map(|handle| (handle, min))
}

/// The screen position of the rotation handle for a pivot.
pub(super) fn rotation_handle_pos(app: &PaintApp, pivot: egui::Pos2) -> egui::Pos2 {
    let angle = app.editor.selection_rotate_angle();
    let direction = egui::vec2(angle.sin(), -angle.cos()) * HANDLE_DIST;
    pivot + direction
}

/// The visible sub-rectangle of the canvas as `[min_x, min_y, size_x, size_y]`
/// in `0..1` uv space.
pub(super) fn visible_uv_rect(canvas: egui::Rect, visible: egui::Rect) -> [f32; 4] {
    let size = canvas.size();
    [
        (visible.min.x - canvas.min.x) / size.x,
        (visible.min.y - canvas.min.y) / size.y,
        visible.width() / size.x,
        visible.height() / size.y,
    ]
}

use super::*;

/// Handles dragging the rotation pivot and the rotation handle.
pub(super) fn handle_selection_rotation(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    magnification: f32,
) -> bool {
    let Some(pivot) = app.editor.selection_pivot() else {
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
    if app.editor.rotating_selection() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.update_selection_rotate(to_image(pos), shift);
        }
        if response.drag_stopped() {
            app.editor.finish_selection_rotate();
        }
        return true;
    }
    if app.editor.pivot_dragging() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.move_selection_pivot(to_image(pos));
        }
        if response.drag_stopped() {
            app.editor.set_pivot_dragging(false);
        }
        return true;
    }
    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if (pos - handle_screen).length() <= CIRCLE_HIT {
                app.editor.begin_selection_rotate(to_image(pos));
                app.editor.update_selection_rotate(to_image(pos), shift);
                return true;
            }
            if (pos - pivot_screen).length() <= CROSS_HIT {
                app.editor.begin_pivot_drag(to_image(pos));
                return true;
            }
        }
    }
    false
}

/// Handles a canvas resize drag; returns whether it consumed the input.
pub(super) fn handle_canvas_resize(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    viewport_rect: egui::Rect,
    magnification: f32,
) -> bool {
    if app.editor.resizing_canvas() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.update_canvas_resize(pos, viewport_rect.min);
        }
        if response.drag_stopped() {
            app.editor.finish_canvas_resize();
        }
        return true;
    }
    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(handle) = origin.and_then(|pos| hovered_resize_handle(canvas_rect, pos)) {
            app.editor
                .begin_canvas_resize(handle, canvas_rect.min, magnification);
            if let Some(pos) = origin {
                app.editor.update_canvas_resize(pos, viewport_rect.min);
            }
            return true;
        }
    }
    false
}

/// Handles a selection-content resize drag; returns whether it consumed input.
pub(super) fn handle_selection_resize(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    magnification: f32,
) -> bool {
    if app.editor.resizing_selection() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.update_selection_resize(pos);
        }
        if response.drag_stopped() {
            app.editor.finish_selection_resize();
        }
        return true;
    }
    if response.drag_started() && ui.input(|i| i.pointer.primary_down()) {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if let Some((handle, start_min)) = selection_handle_at(app, canvas_rect, pos) {
                app.editor
                    .begin_selection_resize(handle, start_min, magnification);
                app.editor.update_selection_resize(pos);
                return true;
            }
        }
    }
    false
}

/// Handles the temporary Ctrl/Alt color-pick while a drawing tool is active.
pub(super) fn handle_pick(
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
            app.editor.pick_color_at(to_canvas(pos), target);
        }
    }
}

/// Handles the Magnifier: drag a rectangle to zoom into it, click to zoom.
pub(super) fn handle_magnifier(
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
                app.editor.begin_zoom_rect(to_canvas(pos));
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.update_zoom_rect(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.editor.apply_zoom_rect(viewport_rect.size());
    }
    if response.clicked() {
        app.editor.zoom_in();
    }
    if response.secondary_clicked() {
        app.editor.zoom_out();
    }
}

pub(super) fn handle_polygon(
    app: &mut PaintApp,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);
    app.editor.set_polygon_preview(
        response
            .hover_pos()
            .filter(|&pos| on_canvas(pos))
            .map(to_canvas),
    );

    if response.double_clicked() {
        app.editor.finish_polygon();
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.editor.polygon_click(to_canvas(pos));
            }
        }
    }
    if response.secondary_clicked() {
        app.editor.finish_polygon();
    }
}

/// Handles the multi-step Curve tool: draw a line, then drag two control points.
pub(super) fn handle_curve(
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
                app.editor.curve_pointer_down(to_canvas(pos));
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.curve_pointer_move(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.editor.curve_pointer_up();
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.editor.curve_pointer_down(to_canvas(pos));
                app.editor.curve_pointer_up();
            }
        }
    }
    if response.secondary_clicked() {
        app.editor.curve_cancel();
    }
}

/// Handles the Text tool: drag to place a box, type, and click outside to commit.
pub(super) fn handle_text(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);

    if app.editor.has_text_box() {
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
            app.editor.text_type(&typed);
        }
        if backspace {
            app.editor.text_backspace();
        }
        if newline {
            app.editor.text_newline();
        }
        app.editor.render_text_preview();

        // A click outside the box commits the text.
        if response.clicked() || response.secondary_clicked() {
            app.editor.commit_text();
        }
        return;
    }

    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos());
        if let Some(pos) = origin {
            if on_canvas(pos) {
                app.editor.begin_text_box(to_canvas(pos));
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.update_text_box(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.editor.finish_text_box();
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.editor.default_text_box(to_canvas(pos));
            }
        }
    }
    if response.secondary_clicked() {
        app.editor.cancel_text();
    }
}

pub(super) fn handle_selection(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let tool = app.editor.settings.tool;
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
                    app.editor.begin_selection_drag(
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
            app.editor.update_selection_drag(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.editor.end_selection_drag();
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let p = to_canvas(pos);
            if shift && on_canvas(pos) {
                // Shift + tap adds a single pixel to the selection.
                app.editor.select_pixel(p, true, false);
            } else if alt && on_canvas(pos) {
                // Alt + tap removes a single pixel from the selection.
                app.editor.select_pixel(p, false, true);
            } else if !on_canvas(pos) || !app.editor.point_in_selection(p) {
                // A plain tap clears the selection instead of leaving a stray
                // 1x1 selection (Shift + tap still adds a single pixel).
                app.editor.deselect();
            }
        }
    }
    if response.secondary_clicked() {
        match response.interact_pointer_pos() {
            // Contour Select drops the connected contour under the cursor.
            Some(pos) if on_canvas(pos) && tool == Tool::ContourSelect => {
                app.editor.contour_deselect_at(to_canvas(pos));
            }
            // Otherwise right-click removes a pixel (anti-selection).
            Some(pos) if on_canvas(pos) => app.editor.select_pixel(to_canvas(pos), false, true),
            _ => app.editor.deselect(),
        }
    }
}

pub(super) fn handle_magic_wand(
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
            if on_canvas(pos) && app.editor.point_in_selection(to_canvas(pos)) {
                app.editor
                    .begin_selection_drag(to_canvas(pos), Tool::Select, false, false);
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.update_selection_drag(to_canvas(pos));
        }
    }
    if response.drag_stopped() {
        app.editor.end_selection_drag();
    }
    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                let p = to_canvas(pos);
                if shift {
                    app.editor.magic_wand_select(p, true, false);
                } else if alt {
                    app.editor.magic_wand_select(p, false, true);
                } else if !app.editor.point_in_selection(p) {
                    app.editor.magic_wand_select(p, false, false);
                }
            }
        }
    }
    if response.secondary_clicked() {
        // Right-click subtracts (anti-selection) from the magic-wand selection.
        match response.interact_pointer_pos() {
            Some(pos) if on_canvas(pos) => {
                app.editor.magic_wand_select(to_canvas(pos), false, true)
            }
            _ => app.editor.deselect(),
        }
    }
}

pub(super) fn handle_selection_brush(
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
                    if !secondary && !alt && !shift && app.editor.point_in_selection(p) {
                        app.editor
                            .begin_selection_drag(p, Tool::Select, false, false);
                    } else {
                        app.editor.selection_brush_begin(p, secondary || alt, shift);
                    }
                }
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let p = to_canvas(pos);
            app.editor.update_selection_drag(p);
            app.editor.selection_brush_to(p);
        }
    }
    if response.drag_stopped() {
        app.editor.end_selection_drag();
        app.editor.selection_brush_end();
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                let p = to_canvas(pos);
                // A plain tap inside the selection does nothing (move territory).
                if shift || alt || !app.editor.point_in_selection(p) {
                    app.editor.selection_brush_begin(p, alt, shift);
                    app.editor.selection_brush_end();
                }
            }
        }
    }
    if response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                app.editor
                    .selection_brush_begin(to_canvas(pos), true, false);
                app.editor.selection_brush_end();
            }
        }
    }
}

pub(super) fn handle_navigation(
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
        app.editor.pan += delta;
    }

    if zoom != 1.0 && over_viewport {
        let anchor = pointer.unwrap_or_else(|| viewport_rect.center());
        let old_mag = app.editor.magnification;
        let new_mag = (old_mag * zoom).clamp(MIN_MAGNIFICATION, MAX_MAGNIFICATION);
        if new_mag != old_mag {
            // Keep the canvas point under the pointer fixed.
            let canvas_min = viewport_rect.min + app.editor.pan;
            let anchored = (anchor - canvas_min) / old_mag;
            app.editor.pan = anchor - viewport_rect.min - anchored * new_mag;
            app.editor.magnification = new_mag;
        }
    }

    if scroll != Vec2::ZERO && over_viewport {
        if shift {
            app.editor.pan.x += scroll.y;
        } else {
            app.editor.pan.y += scroll.y;
        }
    }

    clamp_pan(app, viewport_size);
}

/// Keeps the canvas covering the viewport, centering it when it is smaller.
pub(super) fn clamp_pan(app: &mut PaintApp, viewport_size: Vec2) {
    let content = egui::vec2(
        app.editor.document.width() as f32 * app.editor.magnification,
        app.editor.document.height() as f32 * app.editor.magnification,
    );
    app.editor.pan.x = clamp_axis(app.editor.pan.x, content.x, viewport_size.x);
    app.editor.pan.y = clamp_axis(app.editor.pan.y, content.y, viewport_size.y);
}

pub(super) fn clamp_axis(pan: f32, content: f32, viewport: f32) -> f32 {
    if content <= viewport {
        (viewport - content) / 2.0
    } else {
        pan.clamp(viewport - content, 0.0)
    }
}

pub(super) fn handle_drawing(
    app: &mut PaintApp,
    ui: &egui::Ui,
    response: &egui::Response,
    canvas_rect: egui::Rect,
    to_canvas: &impl Fn(Pos2) -> (i32, i32),
) {
    let shift = ui.input(|i| i.modifiers.shift);
    let on_canvas = |pos: Pos2| canvas_rect.contains(pos);

    // Quick undo: pressing the other mouse button mid-stroke cancels it.
    if app.editor.has_stroke() {
        let (primary, secondary) =
            ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        if primary && secondary {
            app.editor.cancel_stroke();
        }
    }

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
                app.editor
                    .begin_stroke(to_canvas(pos), pointer_button(button));
            }
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.editor.update_stroke(to_canvas(pos), shift);
        }
    }
    if response.drag_stopped() {
        app.editor.end_stroke(shift);
    }

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                let p = to_canvas(pos);
                if app.editor.settings.tool.is_click_tool() {
                    app.editor
                        .click_tool(p, pointer_button(PointerButton::Primary), shift);
                } else if app.editor.settings.tool.is_freehand()
                    || app.editor.settings.tool.is_shape()
                {
                    app.editor
                        .begin_stroke(p, pointer_button(PointerButton::Primary));
                    app.editor.end_stroke(shift);
                }
            }
        }
    }
    if response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if on_canvas(pos) {
                let p = to_canvas(pos);
                if app.editor.settings.tool.is_click_tool() {
                    app.editor
                        .click_tool(p, pointer_button(PointerButton::Secondary), shift);
                } else if app.editor.settings.tool.is_freehand()
                    || app.editor.settings.tool.is_shape()
                {
                    app.editor
                        .begin_stroke(p, pointer_button(PointerButton::Secondary));
                    app.editor.end_stroke(shift);
                }
            }
        }
    }
}

use super::*;

impl Editor {
    pub fn zoom_in(&mut self) {
        self.magnification = (self.magnification * 2.0).min(MAX_MAGNIFICATION);
    }

    pub fn zoom_out(&mut self) {
        self.magnification = (self.magnification / 2.0).max(MIN_MAGNIFICATION);
    }

    pub fn begin_zoom_rect(&mut self, p: (i32, i32)) {
        self.zoom_rect = Some((p, p));
    }

    pub fn update_zoom_rect(&mut self, p: (i32, i32)) {
        if let Some((_, end)) = self.zoom_rect.as_mut() {
            *end = p;
        }
    }

    pub fn zoom_rect(&self) -> Option<((i32, i32), (i32, i32))> {
        self.zoom_rect
    }

    pub fn apply_zoom_rect(&mut self, viewport_size: geometry::Vec2) {
        let Some((a, b)) = self.zoom_rect.take() else {
            return;
        };
        let width = (a.0 - b.0).abs() + 1;
        let height = (a.1 - b.1).abs() + 1;
        if width < 2 || height < 2 {
            return;
        }
        let x = a.0.min(b.0);
        let y = a.1.min(b.1);
        let magnification = (viewport_size.x / width as f32)
            .min(viewport_size.y / height as f32)
            .clamp(MIN_MAGNIFICATION, MAX_MAGNIFICATION);
        self.magnification = magnification;
        let rect_center = geometry::vec2(
            x as f32 + width as f32 / 2.0,
            y as f32 + height as f32 / 2.0,
        );
        self.pan = viewport_size / 2.0 - rect_center * magnification;
    }

    pub fn resizing_canvas(&self) -> bool {
        self.resize_drag.is_some()
    }

    pub fn begin_canvas_resize(
        &mut self,
        handle: ResizeHandle,
        start_min: geometry::Pos2,
        magnification: f32,
    ) {
        self.document.sync_active();
        self.resize_drag = Some(ResizeDrag {
            handle,
            orig_layers: self.document.layers.clone(),
            orig_active: self.document.active,
            orig_width: self.document.width(),
            orig_height: self.document.height(),
            start_min,
            magnification,
            sel_orig: self.selection.as_ref().map(|s| (s.x, s.y)),
        });
    }

    pub(super) fn reanchor_selection_after_resize(&mut self, x: i32, y: i32) {
        if let Some(selection) = self.selection.as_mut() {
            selection.x = x;
            selection.y = y;
            selection.base = None;
            selection.floating = false;
            selection.moved = false;
            selection.dragging = false;
        }
        self.selection_undo.clear();
        self.selection_resize = None;
    }

    pub fn update_canvas_resize(&mut self, pointer: geometry::Pos2, viewport_min: geometry::Pos2) {
        let Some(drag) = self.resize_drag.take() else {
            return;
        };
        let mag = drag.magnification;
        let left = drag.start_min.x;
        let right = drag.start_min.x + drag.orig_width as f32 * mag;
        let top = drag.start_min.y;
        let bottom = drag.start_min.y + drag.orig_height as f32 * mag;

        let mut new_width = drag.orig_width;
        let mut new_height = drag.orig_height;
        let mut new_min_x = left;
        let mut new_min_y = top;

        if drag.handle.touches_east() {
            new_width = ((pointer.x - left) / mag).round().clamp(1.0, 100_000.0) as u32;
        } else if drag.handle.touches_west() {
            new_width = ((right - pointer.x) / mag).round().clamp(1.0, 100_000.0) as u32;
            new_min_x = right - new_width as f32 * mag;
        }
        if drag.handle.touches_south() {
            new_height = ((pointer.y - top) / mag).round().clamp(1.0, 100_000.0) as u32;
        } else if drag.handle.touches_north() {
            new_height = ((bottom - pointer.y) / mag).round().clamp(1.0, 100_000.0) as u32;
            new_min_y = bottom - new_height as f32 * mag;
        }

        let offset_x = if drag.handle.touches_west() {
            new_width as i32 - drag.orig_width as i32
        } else {
            0
        };
        let offset_y = if drag.handle.touches_north() {
            new_height as i32 - drag.orig_height as i32
        } else {
            0
        };

        let fill = if self.document.transparent || self.document.layers.len() > 1 {
            Color::TRANSPARENT
        } else {
            self.colors.background
        };
        let mut layers = drag.orig_layers.clone();
        rspaint_core::layer::map_layer_pixels(&mut layers, &mut |buffer| {
            buffer.resized_with_offset(new_width, new_height, offset_x, offset_y, fill)
        });
        self.document.layers = layers;
        self.document.active = drag.orig_active;
        self.document.refresh_buffer();
        self.pan = geometry::vec2(new_min_x - viewport_min.x, new_min_y - viewport_min.y);
        // Keep the selection anchored to its content after the resize.
        if let Some((origin_x, origin_y)) = drag.sel_orig {
            self.reanchor_selection_after_resize(origin_x + offset_x, origin_y + offset_y);
        }
        self.mark_full();
        self.resize_drag = Some(drag);
    }

    pub fn finish_canvas_resize(&mut self) {
        if self.resize_drag.take().is_some() {
            self.commit("Resize Canvas");
        }
    }

    pub fn adjust_zoom(&mut self) {
        let height = self.document.height();
        if self.viewport_size.y > 0.0 && height > 0 {
            let magnification =
                (self.viewport_size.y / height as f32).clamp(MIN_MAGNIFICATION, MAX_MAGNIFICATION);
            self.magnification = magnification;
        }
        self.center_view();
    }

    pub(super) fn center_view(&mut self) {
        let scaled = geometry::vec2(
            self.document.width() as f32 * self.magnification,
            self.document.height() as f32 * self.magnification,
        );
        self.pan = geometry::vec2(
            if scaled.x <= self.viewport_size.x {
                (self.viewport_size.x - scaled.x) / 2.0
            } else {
                0.0
            },
            if scaled.y <= self.viewport_size.y {
                (self.viewport_size.y - scaled.y) / 2.0
            } else {
                0.0
            },
        );
    }

    pub fn toggle_grid(&mut self) {
        self.settings.show_grid = !self.settings.show_grid;
    }

    pub fn set_magnification(&mut self, mag: f32) {
        let mag = mag.clamp(MIN_MAGNIFICATION, MAX_MAGNIFICATION);
        if (mag - self.magnification).abs() < f32::EPSILON {
            return;
        }
        self.pan *= mag / self.magnification;
        self.magnification = mag;
    }

    pub fn zoom_to_window(&mut self) {
        let (width, height) = (self.document.width(), self.document.height());
        if self.viewport_size.x > 0.0 && self.viewport_size.y > 0.0 && width > 0 && height > 0 {
            let margin = 30.0;
            let mag = ((self.viewport_size.x - margin) / width as f32)
                .min((self.viewport_size.y - margin) / height as f32);
            // Round down to an integer percent, as the Custom Zoom dialog expects.
            let mag = (100.0 * mag).floor() / 100.0;
            self.magnification = mag.clamp(MIN_MAGNIFICATION, MAX_MAGNIFICATION);
        }
        self.center_view();
    }

    pub fn set_viewport_size(&mut self, size: geometry::Vec2) {
        self.viewport_size = size;
        // On the first frame with a known viewport, fit the default sheet.
        if self.pending_adjust_zoom && size.y > 0.0 {
            self.adjust_zoom();
            self.pending_adjust_zoom = false;
        }
    }
}

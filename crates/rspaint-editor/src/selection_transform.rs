use super::*;

impl Editor {
    pub fn selection_bounds(&self) -> Option<(i32, i32, u32, u32)> {
        let mut bounds: Option<(i32, i32, i32, i32)> = None;
        for selection in [self.selection.as_ref(), self.selection_rest.as_ref()]
            .into_iter()
            .flatten()
        {
            let x0 = selection.x;
            let y0 = selection.y;
            let x1 = x0 + selection.pixels.width() as i32;
            let y1 = y0 + selection.pixels.height() as i32;
            bounds = Some(match bounds {
                Some((min_x, min_y, max_x, max_y)) => {
                    (min_x.min(x0), min_y.min(y0), max_x.max(x1), max_y.max(y1))
                }
                None => (x0, y0, x1, y1),
            });
        }
        bounds.map(|(x0, y0, x1, y1)| (x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    /// Whether the active selection is a freshly pasted element.
    ///
    /// Such a selection keeps its transform controls visible and moves its
    /// content on a plain drag (as if Ctrl was still held) until it is dropped.
    pub fn selection_is_pasted(&self) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|selection| selection.pasted)
    }

    pub fn selection_is_simple_rect(&self) -> bool {
        self.selection_rest.is_none()
            && self.selection.as_ref().is_some_and(|selection| {
                selection.outline.is_empty() && selection.mask_bits.iter().all(|&selected| selected)
            })
    }

    pub fn resizing_selection(&self) -> bool {
        self.selection_resize.is_some()
    }

    pub fn begin_selection_resize(
        &mut self,
        handle: ResizeHandle,
        start_min: geometry::Pos2,
        magnification: f32,
    ) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        let fill = self.selection_fill();
        let base = match selection.base.as_ref() {
            Some(base) => base.clone(),
            None => {
                let mut base = self.document.buffer.clone();
                rspaint_core::selection::clear_masked(
                    &mut base,
                    &selection.pixels,
                    selection.x,
                    selection.y,
                    background,
                    fill,
                );
                base
            }
        };
        // Ctrl+Z restores the selection and canvas before the resize.
        self.selection_undo.push((
            Some(selection.clone()),
            self.document.buffer.clone(),
            self.selection_pivot,
        ));
        let base_for_selection = base.clone();
        self.selection_resize = Some(SelectionResizeDrag {
            handle,
            orig_pixels: selection.pixels.clone(),
            orig_mask: selection.mask_bits.clone(),
            orig_x: selection.x,
            orig_y: selection.y,
            orig_w: selection.pixels.width(),
            orig_h: selection.pixels.height(),
            base,
            start_min,
            magnification,
        });
        if let Some(selection) = self.selection.as_mut() {
            selection.floating = true;
            selection.base = Some(base_for_selection);
            selection.moved = true;
            selection.dragging = false;
            selection.name = "Resize Selection";
        }
        self.mark_full();
    }

    pub fn update_selection_resize(&mut self, pointer: geometry::Pos2) {
        let Some(drag) = self.selection_resize.take() else {
            return;
        };
        let mag = drag.magnification;
        let left = drag.start_min.x;
        let right = drag.start_min.x + drag.orig_w as f32 * mag;
        let top = drag.start_min.y;
        let bottom = drag.start_min.y + drag.orig_h as f32 * mag;

        let mut new_width = drag.orig_w;
        let mut new_height = drag.orig_h;
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

        let new_x = drag.orig_x + ((new_min_x - left) / mag).round() as i32;
        let new_y = drag.orig_y + ((new_min_y - top) / mag).round() as i32;

        let new_pixels = transform::scale(&drag.orig_pixels, new_width, new_height);
        let mask_source = mask_to_buffer(&drag.orig_mask, drag.orig_w, drag.orig_h);
        let new_mask = buffer_to_mask(&transform::scale(&mask_source, new_width, new_height));
        let is_rect = new_mask.iter().all(|&selected| selected);
        let boundary = if is_rect {
            Vec::new()
        } else {
            rspaint_core::selection::boundary_segments(&new_mask, new_width, new_height)
        };
        let runs = runs_from_mask(&new_mask, new_width, new_height);

        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        let mut buffer = drag.base.clone();
        rspaint_core::selection::stamp(&mut buffer, &new_pixels, new_x, new_y, background);
        self.document.buffer = buffer;
        if let Some(selection) = self.selection.as_mut() {
            selection.pixels = new_pixels;
            selection.mask_bits = new_mask;
            selection.boundary = boundary;
            selection.runs = runs;
            selection.outline = Vec::new();
            selection.mask = !is_rect;
            selection.x = new_x;
            selection.y = new_y;
            selection.floating = true;
            selection.moved = true;
            selection.dragging = false;
            selection.base = Some(drag.base.clone());
            selection.rotation_origin = None;
            selection.rotation = 0.0;
            selection.name = "Resize Selection";
        }
        self.mark_full();
        self.selection_resize = Some(drag);
    }

    pub fn finish_selection_resize(&mut self) {
        self.selection_resize = None;
    }

    pub fn selection_pivot(&self) -> Option<(f32, f32)> {
        if let Some(pivot) = self.selection_pivot {
            return Some(pivot);
        }
        let (x, y, w, h) = self.selection_bounds()?;
        Some((x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0))
    }

    pub fn move_selection_pivot(&mut self, p: (f32, f32)) {
        self.selection_pivot = Some(p);
    }

    pub fn begin_pivot_drag(&mut self, p: (f32, f32)) {
        if self.selection.is_none() {
            return;
        }
        self.selection_undo.push((
            self.selection.clone(),
            self.document.buffer.clone(),
            self.selection_pivot,
        ));
        self.selection_pivot_dragging = true;
        self.selection_pivot = Some(p);
        // Moving the pivot resets the rotation handle to its default (up).
        if let Some(selection) = self.selection.as_mut() {
            selection.rotation = 0.0;
            selection.rotation_origin = None;
        }
    }

    pub fn pivot_dragging(&self) -> bool {
        self.selection_pivot_dragging
    }

    pub fn set_pivot_dragging(&mut self, dragging: bool) {
        self.selection_pivot_dragging = dragging;
    }

    pub fn selection_rotate_angle(&self) -> f32 {
        self.selection
            .as_ref()
            .map(|selection| selection.rotation)
            .unwrap_or(0.0)
    }

    pub fn rotating_selection(&self) -> bool {
        self.selection_rotate.is_some()
    }

    pub fn begin_selection_rotate(&mut self, pointer: (f32, f32)) {
        if self.selection.is_none() {
            return;
        }
        let Some(pivot) = self.selection_pivot() else {
            return;
        };
        self.selection_undo.push((
            self.selection.clone(),
            self.document.buffer.clone(),
            self.selection_pivot,
        ));
        if self
            .selection
            .as_ref()
            .is_some_and(|selection| selection.rotation_origin.is_none())
        {
            let background = if self.settings.selection_transparent {
                Some(self.colors.background)
            } else {
                None
            };
            let fill = self.selection_fill();
            let (origin_pixels, origin_mask, origin_x, origin_y, base) = {
                let selection = self.selection.as_ref().expect("checked above");
                let base = match selection.base.as_ref() {
                    Some(base) => base.clone(),
                    None => {
                        let mut base = self.document.buffer.clone();
                        rspaint_core::selection::clear_masked(
                            &mut base,
                            &selection.pixels,
                            selection.x,
                            selection.y,
                            background,
                            fill,
                        );
                        base
                    }
                };
                (
                    selection.pixels.clone(),
                    selection.mask_bits.clone(),
                    selection.x,
                    selection.y,
                    base,
                )
            };
            if let Some(selection) = self.selection.as_mut() {
                selection.rotation_origin = Some((origin_pixels, origin_mask, origin_x, origin_y));
                selection.base = Some(base);
                selection.floating = true;
                selection.moved = true;
                selection.dragging = false;
                selection.rotation = 0.0;
                selection.name = "Rotate Selection";
            }
            // Freeze the pivot at the first rotation so it stays fixed.
            self.selection_pivot = Some(pivot);
        }
        let start_angle = angle_from_up(pivot, pointer);
        let base_angle = self
            .selection
            .as_ref()
            .map(|selection| selection.rotation)
            .unwrap_or(0.0);
        self.selection_rotate = Some(SelectionRotateDrag {
            start_angle,
            base_angle,
        });
        self.mark_full();
    }

    pub fn update_selection_rotate(&mut self, pointer: (f32, f32), snap: bool) {
        let Some(drag) = self.selection_rotate.take() else {
            return;
        };
        let Some(pivot) = self.selection_pivot() else {
            self.selection_rotate = Some(drag);
            return;
        };
        // Snap the total angle to 45° increments (like the Line tool).
        let mut total = drag.base_angle + (angle_from_up(pivot, pointer) - drag.start_angle);
        if snap {
            let step = std::f32::consts::FRAC_PI_4;
            total = (total / step).round() * step;
        }

        // Rotate from the original (unrotated) content.
        let origin = self
            .selection
            .as_mut()
            .and_then(|selection| selection.rotation_origin.take());
        let Some((origin_pixels, origin_mask, origin_x, origin_y)) = origin else {
            self.selection_rotate = Some(drag);
            return;
        };
        let (new_pixels, new_mask, (new_x, new_y)) = rotate_content_around(
            &origin_pixels,
            &origin_mask,
            (origin_x, origin_y),
            pivot,
            f64::from(total.to_degrees()),
        );
        let width = new_pixels.width();
        let height = new_pixels.height();
        let is_rect = new_mask.iter().all(|&selected| selected);
        let boundary = if is_rect {
            Vec::new()
        } else {
            rspaint_core::selection::boundary_segments(&new_mask, width, height)
        };
        let runs = runs_from_mask(&new_mask, width, height);
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        let base = self
            .selection
            .as_ref()
            .and_then(|selection| selection.base.clone());
        let mut buffer = base.unwrap_or_else(|| self.document.buffer.clone());
        rspaint_core::selection::stamp(&mut buffer, &new_pixels, new_x, new_y, background);
        self.document.buffer = buffer;
        if let Some(selection) = self.selection.as_mut() {
            selection.rotation_origin = Some((origin_pixels, origin_mask, origin_x, origin_y));
            selection.pixels = new_pixels;
            selection.mask_bits = new_mask;
            selection.boundary = boundary;
            selection.runs = runs;
            selection.outline = Vec::new();
            selection.mask = !is_rect;
            selection.x = new_x;
            selection.y = new_y;
            selection.floating = true;
            selection.moved = true;
            selection.dragging = false;
            selection.rotation = total;
            selection.name = "Rotate Selection";
        }
        self.mark_full();
        self.selection_rotate = Some(drag);
    }

    pub fn finish_selection_rotate(&mut self) {
        self.selection_rotate = None;
    }

    pub fn delete_selection(&mut self) {
        self.delete_selection_named("Delete Selection");
    }

    pub(super) fn delete_selection_named(&mut self, name: &str) {
        if self.selection.is_none() || !self.can_paint_active() {
            return;
        }
        self.selection_undo.push((
            self.selection.clone(),
            self.document.buffer.clone(),
            self.selection_pivot,
        ));
        let Some(selection) = self.selection.take() else {
            return;
        };
        self.creating_selection = None;
        let fill = self.selection_fill();
        self.note_transparency(fill);
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        rspaint_core::selection::clear_masked(
            &mut self.document.buffer,
            &selection.pixels,
            selection.x,
            selection.y,
            background,
            fill,
        );
        self.mark_rect(RectI::between(
            (selection.x, selection.y),
            (
                selection.x + selection.pixels.width() as i32 - 1,
                selection.y + selection.pixels.height() as i32 - 1,
            ),
        ));
        self.commit(name);
    }
}

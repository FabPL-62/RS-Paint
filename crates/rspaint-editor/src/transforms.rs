use super::*;

impl Editor {
    /// A new document must not inherit previews or drag snapshots from the old one.
    fn clear_interaction(&mut self) {
        self.stroke = None;
        self.stroke_painted.clear();
        self.selection = None;
        self.selection_rest = None;
        self.selection_drag_moves_content = true;
        self.creating_selection = None;
        self.creating_selection_add = false;
        self.creating_selection_subtract = false;
        self.selection_undo.clear();
        self.resize_drag = None;
        self.selection_resize = None;
        self.selection_rotate = None;
        self.selection_pivot = None;
        self.selection_pivot_dragging = false;
        self.zoom_rect = None;
        self.polygon_points.clear();
        self.polygon_preview = None;
        self.curve_points.clear();
        self.curve_base = None;
        self.curve_dragging = false;
        self.text_box = None;
        self.text_buffer.clear();
        self.text_creating = None;
        self.text_base = None;
        self.magic_wand_seed = None;
        self.sel_brush_mask.clear();
        self.sel_brush_tiles.clear();
        self.sel_brush_tile_cols = 0;
        self.sel_brush_active = false;
        self.sel_brush_last = None;
        self.pointer = None;
        self.pick_hold = None;
    }

    pub fn reset_document(&mut self) {
        self.document = Document::default();
        self.colors = DocumentColors::default();
        self.history = History::new(self.document.buffer.clone(), "New");
        self.clear_interaction();
        self.file_path = None;
        self.adjust_zoom();
        self.mark_full();
    }

    pub fn undo(&mut self) {
        // Undo the most recent selection operation first.
        if let Some((mut selection, buffer, pivot)) = self.selection_undo.pop() {
            if let Some(selection) = selection.as_mut() {
                selection.dragging = false;
            }
            self.document.buffer = buffer;
            self.selection = selection;
            self.selection_pivot = pivot;
            self.creating_selection = None;
            self.creating_selection_add = false;
            self.creating_selection_subtract = false;
            self.mark_full();
            return;
        }
        if self.history.undo() {
            self.document.buffer = self.history.current_snapshot().clone();
            self.document.modified = true;
            self.mark_full();
        }
    }

    pub fn redo(&mut self) {
        self.selection_undo.clear();
        if self.history.redo() {
            self.document.buffer = self.history.current_snapshot().clone();
            self.document.modified = true;
            self.mark_full();
        }
    }

    pub fn jump_to_history(&mut self, id: rspaint_core::NodeId) {
        if self.history.jump_to(id) {
            self.document.buffer = self.history.current_snapshot().clone();
            self.document.modified = true;
            self.mark_full();
        }
    }

    pub fn commit(&mut self, name: &str) {
        self.history
            .commit(name.to_string(), self.document.buffer.clone());
        self.document.modified = true;
    }

    pub fn set_pointer(&mut self, pointer: Option<(i32, i32)>) {
        self.pointer = pointer;
    }

    pub fn load_buffer(&mut self, buffer: PixelBuffer, path: Option<std::path::PathBuf>) {
        let transparent = buffer.data().chunks_exact(4).any(|pixel| pixel[3] < 255);
        self.colors = DocumentColors::default();
        self.colors.background = buffer.get(0, 0).unwrap_or(Color::WHITE);
        self.document.buffer = buffer;
        self.document.transparent = transparent;
        self.document.modified = false;
        self.document.name = path
            .as_deref()
            .and_then(file_stem)
            .unwrap_or_else(|| String::from("Untitled"));
        self.file_path = path;
        self.history = History::new(self.document.buffer.clone(), "Open");
        self.clear_interaction();
        self.adjust_zoom();
        self.mark_full();
    }

    pub fn replace_primary_with_secondary(&mut self) {
        let from = self.colors.foreground;
        let to = self.colors.background;
        if from == to {
            return;
        }

        if let Some(selection) = self.selection.as_ref() {
            let (ox, oy) = (selection.x, selection.y);
            let (w, h) = (selection.pixels.width(), selection.pixels.height());
            // Update the canvas within the selection...
            for ly in 0..h {
                for lx in 0..w {
                    if !selection
                        .mask_bits
                        .get((ly * w + lx) as usize)
                        .copied()
                        .unwrap_or(true)
                    {
                        continue;
                    }
                    let cx = ox + lx as i32;
                    let cy = oy + ly as i32;
                    if cx >= 0
                        && cy >= 0
                        && self.document.buffer.get(cx as u32, cy as u32) == Some(from)
                    {
                        self.document.buffer.set(cx as u32, cy as u32, to);
                    }
                }
            }
            // ...and the selection's floating content.
            if let Some(selection) = self.selection.as_mut() {
                for ly in 0..h {
                    for lx in 0..w {
                        if selection
                            .mask_bits
                            .get((ly * w + lx) as usize)
                            .copied()
                            .unwrap_or(true)
                            && selection.pixels.get(lx, ly) == Some(from)
                        {
                            selection.pixels.set(lx, ly, to);
                        }
                    }
                }
            }
        } else {
            rspaint_core::fill::replace_color(&mut self.document.buffer, from, to);
        }

        self.mark_full();
        self.commit("Replace Color");
    }

    pub fn apply_outliner(&mut self) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let rx = selection.x;
        let ry = selection.y;
        let rw = selection.pixels.width() as i32;
        let rh = selection.pixels.height() as i32;
        if rw < 1 || rh < 1 {
            return;
        }

        let bg = self.colors.background;
        let fg = self.colors.foreground;
        let extra = self.settings.outliner_color;
        let size = self.settings.outliner_size.max(1) as i32;
        let concave = self.settings.outliner_concave;
        let (outward, inward) = match self.settings.outliner_placement {
            BorderPlacement::Outside => (size, 0),
            BorderPlacement::OneInward => (size.saturating_sub(1), 1),
            BorderPlacement::Symmetric => ((size + 1) / 2, size / 2),
        };

        let region = rspaint_core::selection::extract_rect(
            &self.document.buffer,
            rx,
            ry,
            rw as u32,
            rh as u32,
        );
        let in_rect = |lx: i32, ly: i32| lx >= 0 && ly >= 0 && lx < rw && ly < rh;
        let is_bg =
            |lx: i32, ly: i32| in_rect(lx, ly) && region.get(lx as u32, ly as u32) == Some(bg);
        let is_obj =
            |lx: i32, ly: i32| in_rect(lx, ly) && region.get(lx as u32, ly as u32) != Some(bg);
        let area = (rw * rh) as usize;
        let index = |lx: i32, ly: i32| (ly * rw + lx) as usize;
        let orth = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)];

        // Outside region: 4-connected flood fill of the background color from
        // the rectangle's edges.
        let mut outside = vec![false; area];
        let mut stack: Vec<(i32, i32)> = Vec::new();
        for lx in 0..rw {
            for ly in [0, rh - 1] {
                if is_bg(lx, ly) {
                    stack.push((lx, ly));
                }
            }
        }
        for ly in 0..rh {
            for lx in [0, rw - 1] {
                if is_bg(lx, ly) {
                    stack.push((lx, ly));
                }
            }
        }
        while let Some((lx, ly)) = stack.pop() {
            if !is_bg(lx, ly) {
                continue;
            }
            let i = index(lx, ly);
            if outside[i] {
                continue;
            }
            outside[i] = true;
            for (dx, dy) in orth {
                stack.push((lx + dx, ly + dy));
            }
        }

        let mut painted: Vec<Option<Color>> = vec![None; area];

        // Outward layers: background pixels, starting from the contour.
        let mut prev: Vec<bool> = vec![false; area];
        for layer in 0..outward {
            let mut current = vec![false; area];
            for ly in 0..rh {
                for lx in 0..rw {
                    let i = index(lx, ly);
                    if painted[i].is_some() || !is_bg(lx, ly) {
                        continue;
                    }
                    let adjacent = if layer == 0 {
                        outside[i] && orth.iter().any(|&(dx, dy)| is_obj(lx + dx, ly + dy))
                    } else {
                        orth.iter().any(|&(dx, dy)| {
                            in_rect(lx + dx, ly + dy) && prev[index(lx + dx, ly + dy)]
                        })
                    };
                    if adjacent {
                        painted[i] = Some(fg);
                        current[i] = true;
                    }
                }
            }
            prev = current;
        }

        // Inward layers: object pixels, starting from the contour's object side.
        let mut prev: Vec<bool> = vec![false; area];
        for layer in 0..inward {
            let mut current = vec![false; area];
            for ly in 0..rh {
                for lx in 0..rw {
                    let i = index(lx, ly);
                    if painted[i].is_some() || !is_obj(lx, ly) {
                        continue;
                    }
                    let adjacent = if layer == 0 {
                        orth.iter().any(|&(dx, dy)| {
                            is_bg(lx + dx, ly + dy)
                                && in_rect(lx + dx, ly + dy)
                                && outside[index(lx + dx, ly + dy)]
                        })
                    } else {
                        orth.iter().any(|&(dx, dy)| {
                            in_rect(lx + dx, ly + dy) && prev[index(lx + dx, ly + dy)]
                        })
                    };
                    if adjacent {
                        painted[i] = Some(fg);
                        current[i] = true;
                    }
                }
            }
            prev = current;
        }

        // Concave corners pass (after the last layer): background pixels
        // adjacent to the painted border that sit in a concave corner get the
        // extra color.
        if concave {
            // Snapshot of the border only, so the extra-color pixels painted
            // here don't count as border and don't cascade.
            let border: Vec<bool> = painted.iter().map(|p| p.is_some()).collect();
            for ly in 0..rh {
                for lx in 0..rw {
                    let i = index(lx, ly);
                    // A background pixel on the outside of the border that has
                    // two orthogonal neighbors of the border color (a concave
                    // corner) gets the extra color.
                    if !is_bg(lx, ly) || painted[i].is_some() || !outside[i] {
                        continue;
                    }
                    let up = in_rect(lx, ly - 1) && border[index(lx, ly - 1)];
                    let down = in_rect(lx, ly + 1) && border[index(lx, ly + 1)];
                    let left = in_rect(lx - 1, ly) && border[index(lx - 1, ly)];
                    let right = in_rect(lx + 1, ly) && border[index(lx + 1, ly)];
                    if (up || down) && (left || right) {
                        painted[i] = Some(extra);
                    }
                }
            }
        }

        let mut changed = false;
        for ly in 0..rh {
            for lx in 0..rw {
                if let Some(color) = painted[index(lx, ly)] {
                    let x = rx + lx;
                    let y = ry + ly;
                    if x >= 0
                        && y >= 0
                        && self.document.buffer.get(x as u32, y as u32) != Some(color)
                    {
                        self.document.buffer.paint(
                            x as u32,
                            y as u32,
                            color,
                            self.settings.paint_blend,
                        );
                        changed = true;
                    }
                }
            }
        }

        if changed {
            self.mark_full();
            self.commit("Outliner");
        }
    }

    pub fn invert_colors(&mut self) {
        let inverted = transform::invert(&self.document.buffer);
        self.document.buffer = inverted;
        self.mark_full();
        self.commit("Invert Colors");
    }

    pub(super) fn transform_selection<F>(&mut self, apply: F, name: &'static str)
    where
        F: Fn(&PixelBuffer) -> PixelBuffer,
    {
        let Some(selection) = self.selection.take() else {
            return;
        };
        // Ctrl+Z restores the selection and canvas before the transform.
        self.selection_undo.push((
            Some(selection.clone()),
            self.document.buffer.clone(),
            self.selection_pivot,
        ));
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        let fill = self.selection_fill();

        // Canvas with the selection region cleared, so the transformed content
        // can be stamped fresh (reusing the existing base when already floating).
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

        let new_pixels = apply(&selection.pixels);
        let mask_source = mask_to_buffer(
            &selection.mask_bits,
            selection.pixels.width(),
            selection.pixels.height(),
        );
        let new_mask_bits = buffer_to_mask(&apply(&mask_source));
        let width = new_pixels.width();
        let height = new_pixels.height();
        let is_rect = new_mask_bits.iter().all(|&selected| selected);
        let boundary = if is_rect {
            Vec::new()
        } else {
            rspaint_core::selection::boundary_segments(&new_mask_bits, width, height)
        };
        let runs = runs_from_mask(&new_mask_bits, width, height);

        let (x, y) = (selection.x, selection.y);
        let mut buffer = base.clone();
        rspaint_core::selection::stamp(&mut buffer, &new_pixels, x, y, background);
        self.document.buffer = buffer;
        self.selection = Some(Selection {
            pixels: new_pixels,
            x,
            y,
            outline: Vec::new(),
            mask: !is_rect,
            boundary,
            mask_bits: new_mask_bits,
            runs,
            floating: true,
            dragging: false,
            moved: true,
            drag_offset: (0, 0),
            base: Some(base),
            rotation_origin: None,
            rotation: 0.0,
            pasted: selection.pasted,
            name,
        });
        // The transform stays a floating selection (like a move); it is
        // committed to history when the selection is deselected.
        self.mark_full();
    }

    pub fn crop_to_selection(&mut self) {
        let Some((x, y, width, height)) = self
            .selection
            .as_ref()
            .map(|s| (s.x, s.y, s.pixels.width(), s.pixels.height()))
        else {
            return;
        };
        self.selection = None;
        self.creating_selection = None;
        let cropped = transform::crop(&self.document.buffer, x, y, width, height);
        self.document.buffer = cropped;
        self.pan = geometry::Vec2::ZERO;
        self.mark_full();
        self.commit("Crop");
    }

    pub(super) fn mark_full(&mut self) {
        self.canvas_dirty = true;
        self.dirty_rect = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn mark_rect(&mut self, rect: RectI) {
        self.dirty_rect = Some(match self.dirty_rect {
            Some(existing) => existing.union(rect),
            None => rect,
        });
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn mark_segment(&mut self, a: (i32, i32), b: (i32, i32), radius: i32) {
        self.mark_rect(RectI::between(a, b).expand(radius));
    }

    pub fn take_pending_upload(&mut self) -> Option<Upload> {
        let width = self.document.width();
        let height = self.document.height();

        if self.canvas_dirty {
            self.canvas_dirty = false;
            self.dirty_rect = None;
            return Some(Upload {
                x: 0,
                y: 0,
                width,
                height,
                data: self.document.buffer.shared_data(),
            });
        }

        let rect = self.dirty_rect.take()?;
        let (x, y, w, h) = if rect.covers(width, height) {
            (0, 0, width, height)
        } else {
            rect.clamped(width, height)?
        };
        Some(Upload {
            x,
            y,
            width: w,
            height: h,
            data: std::sync::Arc::new(self.document.buffer.region_rgba(x, y, w, h)),
        })
    }

    pub fn resize_document(&mut self, width: u32, height: u32, transparent: bool) {
        let width = width.max(1);
        let height = height.max(1);
        let fill = if transparent {
            Color::TRANSPARENT
        } else {
            self.colors.background
        };
        if width != self.document.width() || height != self.document.height() {
            self.document.buffer = self.document.buffer.resized(width, height, fill);
        }
        if let Some((x, y)) = self.selection.as_ref().map(|s| (s.x, s.y)) {
            self.reanchor_selection_after_resize(x, y);
        }
        self.document.transparent = transparent;
        self.commit("Resize Canvas");
        self.mark_full();
    }

    pub fn flip_rotate(&mut self, choice: FlipRotate, angle: f64) {
        let name = match choice {
            FlipRotate::FlipHorizontal => "Flip Horizontal",
            FlipRotate::FlipVertical => "Flip Vertical",
            FlipRotate::Rotate90 => "Rotate 90°",
            FlipRotate::Rotate180 => "Rotate 180°",
            FlipRotate::Rotate270 => "Rotate 270°",
            FlipRotate::Arbitrary => "Rotate",
        };
        let apply = |buffer: &PixelBuffer| match choice {
            FlipRotate::FlipHorizontal => transform::flip_horizontal(buffer),
            FlipRotate::FlipVertical => transform::flip_vertical(buffer),
            FlipRotate::Rotate90 => transform::rotate_90(buffer),
            FlipRotate::Rotate180 => transform::rotate_180(buffer),
            FlipRotate::Rotate270 => transform::rotate_270(buffer),
            FlipRotate::Arbitrary => transform::rotate(buffer, angle),
        };
        if self.selection.is_some() {
            self.transform_selection(apply, name);
        } else {
            self.document.buffer = apply(&self.document.buffer);
            self.pan = geometry::Vec2::ZERO;
            self.mark_full();
            self.commit(name);
        }
    }

    pub fn stretch_skew(&mut self, stretch_x: f64, stretch_y: f64, skew_h: f64, skew_v: f64) {
        let x_scale = (stretch_x / 100.0).max(0.01);
        let y_scale = (stretch_y / 100.0).max(0.01);
        let h_skew = skew_h.to_radians();
        let v_skew = skew_v.to_radians();
        let fill = if self.document.transparent {
            Color::TRANSPARENT
        } else {
            self.colors.background
        };
        let stretched = stretch_x != 100.0 || stretch_y != 100.0;
        let skewed = skew_h != 0.0 || skew_v != 0.0;
        let name = match (stretched, skewed) {
            (true, true) => "Stretch and Skew",
            (true, false) => "Stretch",
            (false, true) => "Skew",
            (false, false) => "Stretch and Skew",
        };
        let apply = |buffer: &PixelBuffer| {
            transform::stretch_and_skew(buffer, x_scale, y_scale, h_skew, v_skew, fill)
        };
        if self.selection.is_some() {
            self.transform_selection(apply, name);
        } else {
            self.document.buffer = apply(&self.document.buffer);
            self.pan = geometry::Vec2::ZERO;
            self.mark_full();
            self.commit(name);
        }
    }
}

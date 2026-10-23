use super::*;

fn outline_of(selection: &Selection) -> Option<SelectionOutline> {
    if selection.mask {
        return None;
    }
    if selection.outline.is_empty() {
        Some(SelectionOutline::Rect {
            x: selection.x,
            y: selection.y,
            width: selection.pixels.width(),
            height: selection.pixels.height(),
        })
    } else {
        Some(SelectionOutline::Poly(
            selection
                .outline
                .iter()
                .map(|&(ox, oy)| (selection.x + ox, selection.y + oy))
                .collect(),
        ))
    }
}

fn mask_contains(mask_bits: &[bool], width: u32, height: u32, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 || x as u32 >= width || y as u32 >= height {
        return false;
    }
    mask_bits
        .get((y as u32 * width + x as u32) as usize)
        .copied()
        .unwrap_or(true)
}

fn point_in_selection_part(selection: &Selection, p: (i32, i32)) -> bool {
    let lx = (p.0 as i64) - (selection.x as i64);
    let ly = (p.1 as i64) - (selection.y as i64);
    let width = selection.pixels.width() as i64;
    let height = selection.pixels.height() as i64;
    if lx < 0 || ly < 0 || lx >= width || ly >= height {
        return false;
    }
    let index = (ly * width + lx) as usize;
    selection.mask_bits.get(index).copied().unwrap_or(true)
}

impl Editor {
    pub fn selection_outline(&self) -> Option<SelectionOutline> {
        outline_of(self.selection.as_ref()?)
    }

    pub fn selection_rest_outline(&self) -> Option<SelectionOutline> {
        outline_of(self.selection_rest.as_ref()?)
    }

    pub fn creating_outline(&self) -> Option<SelectionOutline> {
        match self.creating_selection.as_ref()? {
            CreatingSelection::Rect { start, end } => {
                let x = start.0.min(end.0);
                let y = start.1.min(end.1);
                Some(SelectionOutline::Rect {
                    x,
                    y,
                    width: (start.0.max(end.0) - x + 1) as u32,
                    height: (start.1.max(end.1) - y + 1) as u32,
                })
            }
            CreatingSelection::Lasso { points } => Some(SelectionOutline::Poly(points.clone())),
        }
    }

    pub fn selection_boundary(
        &self,
    ) -> Option<(&[rspaint_core::selection::BoundarySegment], (i32, i32))> {
        let selection = self.selection.as_ref()?;
        selection
            .mask
            .then_some((selection.boundary.as_slice(), (selection.x, selection.y)))
    }

    pub fn selection_rest_boundary(
        &self,
    ) -> Option<(&[rspaint_core::selection::BoundarySegment], (i32, i32))> {
        let selection = self.selection_rest.as_ref()?;
        selection
            .mask
            .then_some((selection.boundary.as_slice(), (selection.x, selection.y)))
    }

    pub fn reselect_magic_wand(&mut self) {
        if self.selection.is_some() {
            if let Some(seed) = self.magic_wand_seed {
                self.magic_wand_select(seed, false, false);
            }
        }
    }

    pub fn magic_wand_select(&mut self, p: (i32, i32), add: bool, subtract: bool) {
        self.magic_wand_seed = Some(p);
        let Some((pixels, (x, y), mask_bits)) = rspaint_core::selection::magic_wand(
            &self.document.buffer,
            p.0,
            p.1,
            true,
            self.settings.magic_wand_tolerance,
        ) else {
            return;
        };
        let region = NewRegion {
            pixels,
            x,
            y,
            mask_bits,
            outline: Vec::new(),
            mask: true,
            name: "Magic Wand",
        };
        self.combine_new_region(region, add, subtract);
    }

    pub(super) fn combine_new_region(&mut self, region: NewRegion, add: bool, subtract: bool) {
        if subtract {
            let Some(previous) = self.selection.as_ref() else {
                return;
            };
            if !region_removes_any(previous, &region) {
                return;
            }
            self.selection_undo.push((
                self.selection.clone(),
                self.document.buffer.clone(),
                self.selection_pivot,
            ));
            let previous = self.selection.take().expect("checked above");
            let was_floating = previous.floating;
            let base = previous.base.clone();
            match subtract_region(previous, &region) {
                Some(result) => {
                    if result.floating {
                        if let Some(base) = result.base.clone() {
                            self.document.buffer = base;
                            let background = if self.settings.selection_transparent {
                                Some(self.colors.background)
                            } else {
                                None
                            };
                            rspaint_core::selection::stamp(
                                &mut self.document.buffer,
                                &result.pixels,
                                result.x,
                                result.y,
                                background,
                            );
                        }
                        self.mark_full();
                    }
                    self.selection = Some(result);
                }
                None => {
                    // Nothing left selected; put back the floating pixels.
                    if was_floating {
                        if let Some(base) = base {
                            self.document.buffer = base;
                            self.mark_full();
                        }
                    }
                    self.selection = None;
                }
            }
            return;
        }

        if add {
            // Ignore a region that adds no pixels outside the current selection.
            if let Some(previous) = self.selection.as_ref() {
                if !region_adds_new(previous, &region) {
                    return;
                }
            }
            self.selection_undo.push((
                self.selection.clone(),
                self.document.buffer.clone(),
                self.selection_pivot,
            ));
            if let Some(previous) = self.selection.take() {
                let fill = self.selection_fill();
                let union = union_region(previous, region, fill);
                if union.floating {
                    if let Some(base) = union.base.clone() {
                        self.document.buffer = base;
                        let background = if self.settings.selection_transparent {
                            Some(self.colors.background)
                        } else {
                            None
                        };
                        rspaint_core::selection::stamp(
                            &mut self.document.buffer,
                            &union.pixels,
                            union.x,
                            union.y,
                            background,
                        );
                    }
                    self.mark_full();
                }
                self.selection = Some(union);
                return;
            }
            self.selection = Some(selection_from_region(region));
        } else {
            self.deselect();
            self.selection_undo.push((
                self.selection.clone(),
                self.document.buffer.clone(),
                self.selection_pivot,
            ));
            self.selection = Some(selection_from_region(region));
        }
    }

    pub fn select_pixel(&mut self, p: (i32, i32), add: bool, subtract: bool) {
        if p.0 < 0
            || p.1 < 0
            || p.0 >= self.document.width() as i32
            || p.1 >= self.document.height() as i32
        {
            return;
        }
        let pixels = rspaint_core::selection::extract_rect(&self.document.buffer, p.0, p.1, 1, 1);
        let region = NewRegion {
            pixels,
            x: p.0,
            y: p.1,
            mask_bits: vec![true],
            outline: Vec::new(),
            mask: true,
            name: "Select",
        };
        self.combine_new_region(region, add, subtract);
    }

    pub fn contour_deselect_at(&mut self, p: (i32, i32)) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let (width, height) = (selection.pixels.width(), selection.pixels.height());
        let local_x = p.0 - selection.x;
        let local_y = p.1 - selection.y;
        if local_x < 0 || local_y < 0 || local_x as u32 >= width || local_y as u32 >= height {
            return;
        }
        let index = (local_y as u32 * width + local_x as u32) as usize;
        if !selection.mask_bits.get(index).copied().unwrap_or(false) {
            return;
        }
        self.selection_undo.push((
            self.selection.clone(),
            self.document.buffer.clone(),
            self.selection_pivot,
        ));

        // Flood-fill the connected component (4-connected) and remove it.
        let mut mask_bits = self
            .selection
            .as_ref()
            .map(|selection| selection.mask_bits.clone())
            .unwrap_or_default();
        let mut stack = vec![(local_x, local_y)];
        let mut removed = Vec::new();
        while let Some((x, y)) = stack.pop() {
            if x < 0 || y < 0 || x as u32 >= width || y as u32 >= height {
                continue;
            }
            let index = (y as u32 * width + x as u32) as usize;
            if !mask_bits[index] {
                continue;
            }
            mask_bits[index] = false;
            removed.push((x as u32, y as u32));
            stack.push((x + 1, y));
            stack.push((x - 1, y));
            stack.push((x, y + 1));
            stack.push((x, y - 1));
        }

        if mask_bits.iter().all(|&selected| !selected) {
            self.selection = None;
            self.mark_full();
            return;
        }

        if let Some(selection) = self.selection.as_mut() {
            for (x, y) in removed {
                selection.pixels.set(x, y, Color::TRANSPARENT);
            }
            let is_rect = mask_bits.iter().all(|&selected| selected);
            selection.boundary =
                rspaint_core::selection::boundary_segments(&mask_bits, width, height);
            selection.runs = runs_from_mask(&mask_bits, width, height);
            selection.mask_bits = mask_bits;
            selection.mask = !is_rect;
            selection.outline = Vec::new();
            selection.floating = false;
            selection.base = None;
            selection.moved = false;
            selection.rotation_origin = None;
            selection.rotation = 0.0;
        }
        self.mark_full();
    }

    pub fn select_all(&mut self) {
        let width = self.document.width();
        let height = self.document.height();
        let pixels =
            rspaint_core::selection::extract_rect(&self.document.buffer, 0, 0, width, height);
        let region = NewRegion {
            pixels,
            x: 0,
            y: 0,
            mask_bits: vec![true; (width * height) as usize],
            outline: Vec::new(),
            mask: false,
            name: "Select All",
        };
        self.combine_new_region(region, false, false);
    }

    pub fn point_in_selection(&self, p: (i32, i32)) -> bool {
        [self.selection.as_ref(), self.selection_rest.as_ref()]
            .into_iter()
            .flatten()
            .any(|selection| point_in_selection_part(selection, p))
    }

    /// The size of the selected connected area under `p`, if any.
    ///
    /// A single rectangular or lasso selection reports its whole bounding
    /// box; masks with several disconnected areas report only the area under
    /// the pointer.
    pub fn selection_portion_size(&self, p: (i32, i32)) -> Option<(u32, u32)> {
        for selection in [self.selection.as_ref(), self.selection_rest.as_ref()]
            .into_iter()
            .flatten()
        {
            if !point_in_selection_part(selection, p) {
                continue;
            }
            let width = selection.pixels.width();
            let height = selection.pixels.height();
            if !selection.mask || selection.mask_bits.iter().all(|&selected| selected) {
                return Some((width, height));
            }
            let local_x = p.0 - selection.x;
            let local_y = p.1 - selection.y;
            let mut visited = vec![false; (width * height) as usize];
            let mut stack = vec![(local_x, local_y)];
            let (mut min_x, mut min_y) = (i32::MAX, i32::MAX);
            let (mut max_x, mut max_y) = (i32::MIN, i32::MIN);
            while let Some((x, y)) = stack.pop() {
                if !mask_contains(&selection.mask_bits, width, height, x, y) {
                    continue;
                }
                let index = (y as u32 * width + x as u32) as usize;
                if visited[index] {
                    continue;
                }
                visited[index] = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                stack.push((x + 1, y));
                stack.push((x - 1, y));
                stack.push((x, y + 1));
                stack.push((x, y - 1));
            }
            if max_x < min_x || max_y < min_y {
                return None;
            }
            return Some(((max_x - min_x + 1) as u32, (max_y - min_y + 1) as u32));
        }
        None
    }

    pub(super) fn selection_fill(&self) -> Color {
        // The area a selection is cut/moved from is filled with the secondary
        // color, so its alpha (opaque, partial or fully transparent) is honored.
        self.colors.background
    }

    pub fn begin_selection_drag(
        &mut self,
        p: (i32, i32),
        tool: Tool,
        add: bool,
        subtract: bool,
        move_content: bool,
    ) {
        let combining = add || subtract;
        if !combining && self.point_in_selection(p) {
            self.start_moving_selection(p, move_content);
            return;
        }
        self.creating_selection_add = add;
        self.creating_selection_subtract = subtract;
        if !combining {
            self.deselect();
        }
        self.creating_selection = Some(match tool {
            Tool::FreeFormSelect => CreatingSelection::Lasso { points: vec![p] },
            _ => CreatingSelection::Rect { start: p, end: p },
        });
    }

    pub fn update_selection_drag(&mut self, p: (i32, i32)) {
        if self.selection.as_ref().is_some_and(|s| s.dragging) {
            let offset = self
                .selection
                .as_ref()
                .map(|s| s.drag_offset)
                .unwrap_or((0, 0));
            if !self.selection_drag_moves_content {
                self.move_selection_area_to(p.0 - offset.0, p.1 - offset.1);
                return;
            }
            self.move_selection_to(p.0 - offset.0, p.1 - offset.1);
            return;
        }
        match self.creating_selection.as_mut() {
            Some(CreatingSelection::Rect { end, .. }) => *end = p,
            Some(CreatingSelection::Lasso { points }) => {
                let far_enough = points
                    .last()
                    .is_none_or(|&last| (last.0 - p.0).abs() + (last.1 - p.1).abs() >= 1);
                if far_enough {
                    points.push(p);
                }
            }
            None => {}
        }
    }

    pub fn end_selection_drag(&mut self) {
        let area_only = self
            .selection
            .as_ref()
            .is_some_and(|selection| selection.dragging)
            && !self.selection_drag_moves_content;
        if let Some(selection) = self.selection.as_mut() {
            selection.dragging = false;
        }
        // The independently moved area rejoins the other selected areas.
        if area_only {
            self.merge_selection_rest(false);
            self.refresh_selection_content_from_canvas();
        } else {
            self.merge_selection_rest(true);
        }
        if self.creating_selection.is_some() {
            self.finish_creating_selection();
            // The Outliner applies as soon as the rectangle is drawn.
            if self.settings.tool == Tool::Outliner && self.selection.is_some() {
                self.apply_outliner();
                self.deselect();
            }
        }
    }

    /// Splits the connected area under `p` out of the selection so it can be
    /// moved independently. Returns whether a split happened.
    fn split_selection_component(&mut self, p: (i32, i32), lift_content: bool) -> bool {
        let Some(selection) = self.selection.as_ref() else {
            return false;
        };
        let width = selection.pixels.width();
        let height = selection.pixels.height();
        let local_x = p.0 - selection.x;
        let local_y = p.1 - selection.y;
        if local_x < 0 || local_y < 0 || local_x as u32 >= width || local_y as u32 >= height {
            return false;
        }
        if !mask_contains(&selection.mask_bits, width, height, local_x, local_y) {
            return false;
        }

        let total = (0..height as i32)
            .flat_map(|y| (0..width as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| mask_contains(&selection.mask_bits, width, height, x, y))
            .count();
        let mut component = vec![false; (width * height) as usize];
        let mut stack = vec![(local_x, local_y)];
        let mut count = 0usize;
        while let Some((x, y)) = stack.pop() {
            if !mask_contains(&selection.mask_bits, width, height, x, y) {
                continue;
            }
            let index = (y as u32 * width + x as u32) as usize;
            if component[index] {
                continue;
            }
            component[index] = true;
            count += 1;
            stack.push((x + 1, y));
            stack.push((x - 1, y));
            stack.push((x, y + 1));
            stack.push((x, y - 1));
        }
        if count == total {
            return false;
        }

        let selection = self.selection.take().expect("checked above");

        let fill = self.selection_fill();
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };

        let mut moving = selection.clone();
        let mut rest = selection.clone();
        let mut moving_pixels = PixelBuffer::transparent(width, height);
        let mut rest_pixels = PixelBuffer::transparent(width, height);
        let mut rest_mask = vec![false; (width * height) as usize];
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                if !mask_contains(&selection.mask_bits, width, height, x, y) {
                    continue;
                }
                let index = (y as u32 * width + x as u32) as usize;
                let color = selection
                    .pixels
                    .get(x as u32, y as u32)
                    .unwrap_or(Color::TRANSPARENT);
                if component[index] {
                    moving_pixels.set(x as u32, y as u32, color);
                } else {
                    rest_pixels.set(x as u32, y as u32, color);
                    rest_mask[index] = true;
                }
            }
        }

        moving.pixels = moving_pixels;
        moving.mask_bits = component.clone();
        moving.outline = Vec::new();
        moving.mask = !component.iter().all(|&selected| selected);
        moving.boundary = rspaint_core::selection::boundary_segments(&component, width, height);
        moving.runs = runs_from_mask(&component, width, height);
        moving.dragging = false;
        moving.drag_offset = (0, 0);
        moving.rotation_origin = None;
        moving.rotation = 0.0;

        rest.pixels = rest_pixels;
        rest.mask_bits = rest_mask;
        rest.outline = Vec::new();
        rest.mask = !rest.mask_bits.iter().all(|&selected| selected);
        rest.boundary = rspaint_core::selection::boundary_segments(&rest.mask_bits, width, height);
        rest.runs = runs_from_mask(&rest.mask_bits, width, height);
        rest.dragging = false;
        rest.drag_offset = (0, 0);
        rest.rotation_origin = None;
        rest.rotation = 0.0;

        // Each side keeps only its own area, so crop both to the tight bounds
        // of the pixels they still select (see `tighten_selection`).
        tighten_selection(&mut moving);
        tighten_selection(&mut rest);

        // When the selection was already floating, keep the other areas on the
        // canvas as the moving area is lifted out.
        if lift_content && moving.floating {
            rspaint_core::selection::stamp(
                &mut self.document.buffer,
                &rest.pixels,
                rest.x,
                rest.y,
                background,
            );
            let mut base = self.document.buffer.clone();
            rspaint_core::selection::clear_masked(
                &mut base,
                &moving.pixels,
                moving.x,
                moving.y,
                background,
                fill,
            );
            moving.base = Some(base);
            self.mark_full();
        }

        self.selection = Some(moving);
        self.selection_rest = Some(rest);
        true
    }

    /// Merges the other selected areas back into the moving selection.
    fn merge_selection_rest(&mut self, lift_content: bool) {
        let Some(rest) = self.selection_rest.take() else {
            return;
        };
        let Some(active) = self.selection.take() else {
            self.selection = Some(rest);
            return;
        };
        let fill = self.selection_fill();
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        let mut merged = merge_selections(rest, active);
        if lift_content {
            if merged.floating {
                let mut base = self.document.buffer.clone();
                rspaint_core::selection::clear_masked(
                    &mut base,
                    &merged.pixels,
                    merged.x,
                    merged.y,
                    background,
                    fill,
                );
                let mut buffer = base.clone();
                rspaint_core::selection::stamp(
                    &mut buffer,
                    &merged.pixels,
                    merged.x,
                    merged.y,
                    background,
                );
                self.document.buffer = buffer;
                merged.base = Some(base);
            }
        } else {
            merged.floating = false;
            merged.base = None;
            merged.moved = false;
            merged.rotation_origin = None;
            merged.rotation = 0.0;
        }
        self.selection = Some(merged);
        if lift_content {
            self.mark_full();
        } else {
            self.refresh_selection_content_from_canvas();
        }
    }

    /// Re-reads the content under the current selection area from the canvas,
    /// used after moving only the selection outline.
    fn refresh_selection_content_from_canvas(&mut self) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let width = selection.pixels.width();
        let height = selection.pixels.height();
        let mask_bits = selection.mask_bits.clone();
        let (x, y) = (selection.x, selection.y);
        let mut pixels = PixelBuffer::transparent(width, height);
        for ly in 0..height as i32 {
            for lx in 0..width as i32 {
                if !mask_contains(&mask_bits, width, height, lx, ly) {
                    continue;
                }
                let cx = x + lx;
                let cy = y + ly;
                if cx >= 0 && cy >= 0 {
                    if let Some(color) = self.document.buffer.get(cx as u32, cy as u32) {
                        pixels.set(lx as u32, ly as u32, color);
                    }
                }
            }
        }
        if let Some(selection) = self.selection.as_mut() {
            selection.pixels = pixels;
            selection.floating = false;
            selection.base = None;
            selection.moved = false;
            selection.dragging = false;
            selection.rotation_origin = None;
            selection.rotation = 0.0;
        }
    }

    pub(super) fn start_moving_selection(&mut self, p: (i32, i32), move_content: bool) {
        self.selection_drag_moves_content = move_content;
        if move_content {
            // Snapshot the pre-move state so Ctrl+Z can undo just the move
            // while keeping the selection and every selected area.
            if self
                .selection
                .as_ref()
                .is_some_and(|selection| !selection.moved)
            {
                let snapshot = self.selection.clone();
                self.selection_undo.push((
                    snapshot,
                    self.document.buffer.clone(),
                    self.selection_pivot,
                ));
            }
            // Ctrl moves the whole selection, including every disconnected
            // area. Only the outline-only drag (without Ctrl) splits a single
            // connected area out of a multiselection.
            let fill = self.selection_fill();
            let background = if self.settings.selection_transparent {
                Some(self.colors.background)
            } else {
                None
            };
            let Some(selection) = self.selection.as_mut() else {
                return;
            };
            selection.dragging = true;
            selection.drag_offset = (p.0 - selection.x, p.1 - selection.y);
            selection.moved = true;
            if !selection.floating {
                let mut base = self.document.buffer.clone();
                rspaint_core::selection::clear_masked(
                    &mut base,
                    &selection.pixels,
                    selection.x,
                    selection.y,
                    background,
                    fill,
                );
                selection.base = Some(base);
                selection.floating = true;
            }
        } else {
            // Moving only the outline: persist any floating content first so
            // the pixels stay where they are.
            if self
                .selection
                .as_ref()
                .is_some_and(|selection| selection.floating)
            {
                self.settle_selection();
            }
            if let Some(selection) = self.selection.as_ref() {
                self.selection_undo.push((
                    Some(selection.clone()),
                    self.document.buffer.clone(),
                    self.selection_pivot,
                ));
            }
            self.split_selection_component(p, false);
            if let Some(selection) = self.selection.as_mut() {
                selection.dragging = true;
                selection.drag_offset = (p.0 - selection.x, p.1 - selection.y);
                selection.rotation_origin = None;
                selection.rotation = 0.0;
            }
        }
    }

    /// Moves only the selection outline, leaving the canvas untouched.
    pub(super) fn move_selection_area_to(&mut self, new_x: i32, new_y: i32) {
        let (dx, dy) = self
            .selection
            .as_ref()
            .map(|selection| (new_x - selection.x, new_y - selection.y))
            .unwrap_or((0, 0));
        if let Some(pivot) = self.selection_pivot.as_mut() {
            pivot.0 += dx as f32;
            pivot.1 += dy as f32;
        }
        if let Some(selection) = self.selection.as_mut() {
            if let Some((_, _, origin_x, origin_y)) = selection.rotation_origin.as_mut() {
                *origin_x += dx;
                *origin_y += dy;
            }
            selection.x = new_x;
            selection.y = new_y;
        }
    }

    pub(super) fn move_selection_to(&mut self, new_x: i32, new_y: i32) {
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        // Move the pivot and rotation origin with the selection.
        let (dx, dy) = self
            .selection
            .as_ref()
            .map(|selection| (new_x - selection.x, new_y - selection.y))
            .unwrap_or((0, 0));
        if let Some(pivot) = self.selection_pivot.as_mut() {
            pivot.0 += dx as f32;
            pivot.1 += dy as f32;
        }
        if let Some(selection) = self.selection.as_mut() {
            if let Some((_, _, origin_x, origin_y)) = selection.rotation_origin.as_mut() {
                *origin_x += dx;
                *origin_y += dy;
            }
        }
        let mut dirty = None;
        if let Some(selection) = self.selection.as_mut() {
            let (old_x, old_y) = (selection.x, selection.y);
            let width = selection.pixels.width();
            let height = selection.pixels.height();
            selection.x = new_x;
            selection.y = new_y;

            let union = union_rect(old_x, old_y, new_x, new_y, width, height);
            if let Some((x, y, w, h)) = union.clamped(self.document.width(), self.document.height())
            {
                if let Some(base) = selection.base.as_ref() {
                    self.document.buffer.copy_region_from(base, x, y, w, h);
                }
            }
            rspaint_core::selection::stamp(
                &mut self.document.buffer,
                &selection.pixels,
                new_x,
                new_y,
                background,
            );
            dirty = Some(union);
        }
        if let Some(rect) = dirty {
            self.mark_rect(rect);
        }
    }

    pub(super) fn finish_creating_selection(&mut self) {
        let Some(creating) = self.creating_selection.take() else {
            return;
        };
        let add = self.creating_selection_add;
        let subtract = self.creating_selection_subtract;
        self.creating_selection_add = false;
        self.creating_selection_subtract = false;

        let region: Option<NewRegion> = match creating {
            CreatingSelection::Rect { start, end } => {
                let x = start.0.min(end.0);
                let y = start.1.min(end.1);
                let width = (start.0.max(end.0) - x + 1) as u32;
                let height = (start.1.max(end.1) - y + 1) as u32;
                // Reject only a single pixel; 1px-wide/tall lines are allowed.
                if width < 2 && height < 2 {
                    None
                } else {
                    let pixels = rspaint_core::selection::extract_rect(
                        &self.document.buffer,
                        x,
                        y,
                        width,
                        height,
                    );
                    if self.settings.tool == Tool::ContourSelect {
                        // Select the pixels that differ from the secondary color.
                        let background = self.colors.background;
                        let mut mask_bits = vec![false; (width * height) as usize];
                        let mut content = PixelBuffer::transparent(width, height);
                        for ly in 0..height {
                            for lx in 0..width {
                                if let Some(color) = pixels.get(lx, ly) {
                                    if color != background {
                                        mask_bits[(ly * width + lx) as usize] = true;
                                        content.set(lx, ly, color);
                                    }
                                }
                            }
                        }
                        let is_mask = !mask_bits.iter().all(|&selected| selected);
                        Some(NewRegion {
                            pixels: content,
                            x,
                            y,
                            mask_bits,
                            outline: Vec::new(),
                            mask: is_mask,
                            name: "Contour Select",
                        })
                    } else {
                        Some(NewRegion {
                            pixels,
                            x,
                            y,
                            mask_bits: vec![true; (width * height) as usize],
                            outline: Vec::new(),
                            mask: false,
                            name: "Select",
                        })
                    }
                }
            }
            CreatingSelection::Lasso { points } => {
                rspaint_core::selection::extract_lasso(&self.document.buffer, &points).map(
                    |(pixels, (x, y), mask_bits)| NewRegion {
                        pixels,
                        x,
                        y,
                        mask_bits,
                        // Draw the ants on the actual selected pixels. The raw
                        // pointer path can differ from the rasterized mask and
                        // leave the closing segment outside the selection.
                        outline: Vec::new(),
                        mask: true,
                        name: "Free-Form Select",
                    },
                )
            }
        };

        if let Some(region) = region {
            self.combine_new_region(region, add, subtract);
        }
    }

    pub fn deselect(&mut self) {
        self.creating_selection = None;
        self.creating_selection_add = false;
        self.merge_selection_rest(self.selection_drag_moves_content);
        if let Some(selection) = self.selection.take() {
            if selection.floating && selection.moved {
                self.commit(selection.name);
            }
        }
        // Selection is finalized; its undo stack no longer applies.
        self.selection_undo.clear();
        self.selection_pivot = None;
        self.selection_pivot_dragging = false;
        self.selection_rotate = None;
    }

    pub(super) fn settle_selection(&mut self) {
        self.merge_selection_rest(self.selection_drag_moves_content);
        let commit_name = self.selection.as_ref().and_then(|selection| {
            (selection.floating && selection.moved).then_some(selection.name)
        });
        if let Some(name) = commit_name {
            self.commit(name);
        }
        if let Some(selection) = self.selection.as_mut() {
            selection.floating = false;
            selection.base = None;
            selection.moved = false;
            selection.dragging = false;
        }
        self.selection_undo.clear();
        self.selection_resize = None;
        self.selection_rotate = None;
        self.selection_pivot = None;
        self.selection_pivot_dragging = false;
    }

    pub fn clipping_to_selection(&self) -> bool {
        self.selection.is_some() && self.settings.tool.is_drawing()
    }

    pub(super) fn selection_canvas_mask(&self) -> Option<Vec<bool>> {
        let selection = self.selection.as_ref()?;
        let (width, height) = (self.document.width() as i32, self.document.height() as i32);
        let mut mask = vec![false; (width * height) as usize];
        let (w, h) = (selection.pixels.width(), selection.pixels.height());
        for ly in 0..h {
            for lx in 0..w {
                if selection
                    .mask_bits
                    .get((ly * w + lx) as usize)
                    .copied()
                    .unwrap_or(true)
                {
                    let cx = selection.x + lx as i32;
                    let cy = selection.y + ly as i32;
                    if cx >= 0 && cy >= 0 && cx < width && cy < height {
                        mask[(cy * width + cx) as usize] = true;
                    }
                }
            }
        }
        Some(mask)
    }

    pub(super) fn clip_region_to_selection(&mut self, snapshot: &PixelBuffer, rect: RectI) {
        if !self.clipping_to_selection() {
            return;
        }
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let (sw, sh) = (selection.pixels.width(), selection.pixels.height());
        let (sx, sy) = (selection.x, selection.y);
        let (width, height) = (self.document.width() as i32, self.document.height() as i32);
        for y in rect.y0.max(0)..=rect.y1.min(height - 1) {
            for x in rect.x0.max(0)..=rect.x1.min(width - 1) {
                let lx = x - sx;
                let ly = y - sy;
                let selected = lx >= 0
                    && ly >= 0
                    && (lx as u32) < sw
                    && (ly as u32) < sh
                    && selection
                        .mask_bits
                        .get((ly as u32 * sw + lx as u32) as usize)
                        .copied()
                        .unwrap_or(true);
                if !selected {
                    if let Some(color) = snapshot.get(x as u32, y as u32) {
                        self.document.buffer.set(x as u32, y as u32, color);
                    }
                }
            }
        }
    }

    pub fn paste_pixels(
        &mut self,
        pixels: PixelBuffer,
        mut mask_bits: Vec<bool>,
        outline: Vec<(i32, i32)>,
        mask: bool,
        name: &'static str,
    ) {
        self.deselect();
        let width = pixels.width();
        let height = pixels.height();
        if width == 0 || height == 0 {
            return;
        }
        if mask_bits.len() != (width * height) as usize {
            mask_bits = vec![true; (width * height) as usize];
        }
        let boundary = if mask {
            rspaint_core::selection::boundary_segments(&mask_bits, width, height)
        } else {
            Vec::new()
        };
        let runs = runs_from_mask(&mask_bits, width, height);

        // Paste centered on the pointer (or the canvas center), as a floating
        // selection over the canvas.
        let (px, py) = self.pointer.unwrap_or((
            self.document.width() as i32 / 2,
            self.document.height() as i32 / 2,
        ));
        // Keep the whole pasted image inside the canvas.
        let max_x = (self.document.width() as i32 - width as i32).max(0);
        let max_y = (self.document.height() as i32 - height as i32).max(0);
        let x = (px - width as i32 / 2).clamp(0, max_x);
        let y = (py - height as i32 / 2).clamp(0, max_y);
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        let base = self.document.buffer.clone();
        rspaint_core::selection::stamp(&mut self.document.buffer, &pixels, x, y, background);

        self.set_tool(Tool::Select);
        self.selection = Some(Selection {
            pixels,
            x,
            y,
            outline,
            mask,
            boundary,
            mask_bits,
            runs,
            floating: true,
            dragging: false,
            moved: true,
            drag_offset: (0, 0),
            base: Some(base),
            rotation_origin: None,
            rotation: 0.0,
            name,
        });
        self.mark_full();
    }
}

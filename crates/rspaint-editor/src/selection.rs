use super::*;

impl Editor {
    pub fn selection_outline(&self) -> Option<SelectionOutline> {
        let selection = self.selection.as_ref()?;
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
        let Some(selection) = self.selection.as_ref() else {
            return false;
        };
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

    pub(super) fn selection_fill(&self) -> Color {
        // The area a selection is cut/moved from is filled with the secondary
        // color, so its alpha (opaque, partial or fully transparent) is honored.
        self.colors.background
    }

    pub fn begin_selection_drag(&mut self, p: (i32, i32), tool: Tool, add: bool, subtract: bool) {
        let combining = add || subtract;
        if !combining && self.point_in_selection(p) {
            self.start_moving_selection(p);
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
            // Snapshot once at the start of the move so Ctrl+Z can undo just
            // the move while keeping the selection.
            if self.selection.as_ref().is_some_and(|s| !s.moved) {
                // Snapshot the pre-move state: not floating, so it is
                // consistent with the unchanged canvas.
                let mut snapshot = self.selection.clone();
                if let Some(selection) = snapshot.as_mut() {
                    selection.dragging = false;
                    selection.floating = false;
                    selection.base = None;
                }
                self.selection_undo.push((
                    snapshot,
                    self.document.buffer.clone(),
                    self.selection_pivot,
                ));
            }
            if let Some(selection) = self.selection.as_mut() {
                selection.moved = true;
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
        if let Some(selection) = self.selection.as_mut() {
            selection.dragging = false;
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

    pub(super) fn start_moving_selection(&mut self, p: (i32, i32)) {
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
                    |(pixels, (x, y), mask_bits)| {
                        let outline = points.iter().map(|&(px, py)| (px - x, py - y)).collect();
                        NewRegion {
                            pixels,
                            x,
                            y,
                            mask_bits,
                            outline,
                            mask: false,
                            name: "Free-Form Select",
                        }
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

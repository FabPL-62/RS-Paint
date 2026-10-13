use super::*;

impl Editor {
    pub fn selection_brush_begin(&mut self, p: (i32, i32), subtract: bool, add: bool) {
        self.selection_undo.push((
            self.selection.clone(),
            self.document.buffer.clone(),
            self.selection_pivot,
        ));

        let (width, height) = (self.document.width(), self.document.height());
        let mut mask = vec![false; (width * height) as usize];
        if add || subtract {
            if let Some(selection) = self.selection.as_ref() {
                let (sx, sy) = (selection.x, selection.y);
                for oy in 0..selection.pixels.height() as i32 {
                    for ox in 0..selection.pixels.width() as i32 {
                        let index = (oy as u32 * selection.pixels.width() + ox as u32) as usize;
                        if selection.mask_bits.get(index).copied().unwrap_or(true) {
                            let cx = sx + ox;
                            let cy = sy + oy;
                            if cx >= 0 && cy >= 0 && (cx as u32) < width && (cy as u32) < height {
                                mask[(cy as u32 * width + cx as u32) as usize] = true;
                            }
                        }
                    }
                }
            }
        }
        self.sel_brush_mask = mask;
        self.sel_brush_subtract = subtract;
        self.sel_brush_active = true;
        self.sel_brush_last = Some(p);
        self.selection_brush_stamp(p, p);
    }

    pub fn selection_brush_to(&mut self, p: (i32, i32)) {
        if !self.sel_brush_active {
            return;
        }
        if let Some(last) = self.sel_brush_last {
            self.selection_brush_stamp(last, p);
        }
        self.sel_brush_last = Some(p);
    }

    pub fn selection_brush_end(&mut self) {
        if !self.sel_brush_active {
            return;
        }
        self.sel_brush_active = false;
        self.sel_brush_last = None;
        self.sel_brush_boundary.clear();
        self.sel_brush_runs.clear();

        let (width, height) = (self.document.width(), self.document.height());
        let mask = std::mem::take(&mut self.sel_brush_mask);
        let mut min_x = width as i32;
        let mut min_y = height as i32;
        let mut max_x = -1;
        let mut max_y = -1;
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                if mask[(y as u32 * width + x as u32) as usize] {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if max_x < min_x {
            self.selection = None;
            return;
        }
        let box_w = (max_x - min_x + 1) as u32;
        let box_h = (max_y - min_y + 1) as u32;
        let mut content = PixelBuffer::transparent(box_w, box_h);
        let mut box_mask = vec![false; (box_w * box_h) as usize];
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if mask[(y as u32 * width + x as u32) as usize] {
                    box_mask[((y - min_y) as u32 * box_w + (x - min_x) as u32) as usize] = true;
                    if let Some(color) = self.document.buffer.get(x as u32, y as u32) {
                        content.set((x - min_x) as u32, (y - min_y) as u32, color);
                    }
                }
            }
        }
        let boundary = rspaint_core::selection::boundary_segments(&box_mask, box_w, box_h);
        let runs = runs_from_mask(&box_mask, box_w, box_h);
        self.selection = Some(Selection {
            pixels: content,
            x: min_x,
            y: min_y,
            outline: Vec::new(),
            mask: true,
            boundary,
            mask_bits: box_mask,
            runs,
            floating: false,
            dragging: false,
            moved: false,
            drag_offset: (0, 0),
            base: None,
            rotation_origin: None,
            rotation: 0.0,
            name: "Selection Brush",
        });
    }

    pub(super) fn selection_brush_stamp(&mut self, from: (i32, i32), to: (i32, i32)) {
        let (width, height) = (self.document.width(), self.document.height());
        let brush = Brush::new(self.settings.brush_shape, self.settings.brush_size);
        let subtract = self.sel_brush_subtract;
        let mask = &mut self.sel_brush_mask;
        let stamp = |cx: i32, cy: i32| {
            brush.visit(cx, cy, |px, py| {
                if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
                    mask[(py as u32 * width + px as u32) as usize] = !subtract;
                }
            });
        };
        if brush.size > 1 {
            walk_dense_line(from, to, stamp);
        } else {
            walk_line(from, to, stamp);
        }
        self.sel_brush_boundary =
            rspaint_core::selection::boundary_segments(&self.sel_brush_mask, width, height);
        self.sel_brush_runs = runs_from_mask(&self.sel_brush_mask, width, height);
    }

    pub fn selection_brush_boundary(&self) -> Option<&[rspaint_core::selection::BoundarySegment]> {
        self.sel_brush_active
            .then_some(self.sel_brush_boundary.as_slice())
    }

    pub fn selection_brush_runs(&self) -> Option<&[HighlightRun]> {
        self.sel_brush_active
            .then_some(self.sel_brush_runs.as_slice())
    }

    pub fn selection_runs(&self) -> Option<(&[HighlightRun], (i32, i32))> {
        let selection = self.selection.as_ref()?;
        Some((selection.runs.as_slice(), (selection.x, selection.y)))
    }
}

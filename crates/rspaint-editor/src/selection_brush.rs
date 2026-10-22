use super::*;

/// Side of the mask tiles used to keep Selection Brush previews incremental.
///
/// A stroke only recomputes the tiles its last segment touched, so tracing a
/// large canvas no longer rescans every pixel on each pointer move.
const SEL_BRUSH_TILE: i32 = 64;

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
        self.reset_sel_brush_tiles(width, height);
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
        self.sel_brush_tiles.clear();
        self.sel_brush_tile_cols = 0;

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
        // Only the tiles the stamped segment reaches can change, so recompute
        // just those instead of rescanning the whole mask.
        let margin = brush.size as i32 / 2 + 2;
        let c0 = (from.0.min(to.0) - margin).div_euclid(SEL_BRUSH_TILE);
        let c1 = (from.0.max(to.0) + margin).div_euclid(SEL_BRUSH_TILE);
        let r0 = (from.1.min(to.1) - margin).div_euclid(SEL_BRUSH_TILE);
        let r1 = (from.1.max(to.1) + margin).div_euclid(SEL_BRUSH_TILE);
        self.refresh_sel_brush_tiles(c0, r0, c1, r1);
    }

    /// Builds an empty tile grid for a canvas of the given size and fills it.
    fn reset_sel_brush_tiles(&mut self, width: u32, height: u32) {
        let cols = tiles_along(width);
        let rows = tiles_along(height);
        self.sel_brush_tile_cols = cols;
        self.sel_brush_tiles = (0..(cols * rows) as usize)
            .map(|_| SelectionBrushTile::default())
            .collect();
        self.refresh_sel_brush_tiles(0, 0, cols - 1, rows - 1);
    }

    /// Recomputes the outline and highlight runs of the tiles in the given
    /// (inclusive) tile range, clamped to the grid.
    fn refresh_sel_brush_tiles(&mut self, c0: i32, r0: i32, c1: i32, r1: i32) {
        let cols = self.sel_brush_tile_cols;
        if cols <= 0 {
            return;
        }
        let rows = self.sel_brush_tiles.len() as i32 / cols;
        let (c0, c1) = (c0.max(0), c1.min(cols - 1));
        let (r0, r1) = (r0.max(0), r1.min(rows - 1));
        if c0 > c1 || r0 > r1 {
            return;
        }
        let width = self.document.width() as i32;
        let height = self.document.height() as i32;
        for r in r0..=r1 {
            for c in c0..=c1 {
                let tx = c * SEL_BRUSH_TILE;
                let ty = r * SEL_BRUSH_TILE;
                let tw = SEL_BRUSH_TILE.min(width - tx);
                let th = SEL_BRUSH_TILE.min(height - ty);
                let (boundary, runs) =
                    tile_outline(&self.sel_brush_mask, width, height, tx, ty, tw, th);
                let tile = &mut self.sel_brush_tiles[(r * cols + c) as usize];
                tile.boundary = boundary;
                tile.runs = runs;
            }
        }
    }

    pub fn selection_brush_tiles(&self) -> Option<&[SelectionBrushTile]> {
        self.sel_brush_active
            .then_some(self.sel_brush_tiles.as_slice())
    }

    pub fn selection_runs(&self) -> Option<(&[HighlightRun], (i32, i32))> {
        let selection = self.selection.as_ref()?;
        Some((selection.runs.as_slice(), (selection.x, selection.y)))
    }
}

fn tiles_along(length: u32) -> i32 {
    ((length as i32 + SEL_BRUSH_TILE - 1) / SEL_BRUSH_TILE).max(1)
}

/// The boundary segments and highlight runs of one tile, in absolute canvas
/// coordinates. Segments spanning a tile edge are split there; the marching
/// ants are anchored to screen coordinates, so the split is invisible.
fn tile_outline(
    mask: &[bool],
    width: i32,
    height: i32,
    tx: i32,
    ty: i32,
    tw: i32,
    th: i32,
) -> (
    Vec<rspaint_core::selection::BoundarySegment>,
    Vec<HighlightRun>,
) {
    let selected = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && x < width && y < height && mask[(y * width + x) as usize]
    };
    let (x1, y1) = (tx + tw, ty + th);

    let mut boundary = Vec::new();
    // Horizontal edges, on grid-line rows.
    for y in ty..=y1 {
        let mut x = tx;
        while x < x1 {
            if selected(x, y - 1) != selected(x, y) {
                let start = x;
                while x < x1 && selected(x, y - 1) != selected(x, y) {
                    x += 1;
                }
                boundary.push(((start, y), (x, y)));
            } else {
                x += 1;
            }
        }
    }
    // Vertical edges, on grid-line columns.
    for x in tx..=x1 {
        let mut y = ty;
        while y < y1 {
            if selected(x - 1, y) != selected(x, y) {
                let start = y;
                while y < y1 && selected(x - 1, y) != selected(x, y) {
                    y += 1;
                }
                boundary.push(((x, start), (x, y)));
            } else {
                y += 1;
            }
        }
    }

    let mut runs = Vec::new();
    for y in ty..y1 {
        let mut x = tx;
        while x < x1 {
            if selected(x, y) {
                let start = x;
                while x < x1 && selected(x, y) {
                    x += 1;
                }
                runs.push((y, start, x - 1));
            } else {
                x += 1;
            }
        }
    }

    (boundary, runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn unit_edges(
        segments: &[rspaint_core::selection::BoundarySegment],
    ) -> BTreeSet<((i32, i32), (i32, i32))> {
        let mut edges = BTreeSet::new();
        for &((x0, y0), (x1, y1)) in segments {
            if y0 == y1 {
                for x in x0.min(x1)..x0.max(x1) {
                    edges.insert(((x, y0), (x + 1, y0)));
                }
            } else {
                for y in y0.min(y1)..y0.max(y1) {
                    edges.insert(((x0, y), (x0, y + 1)));
                }
            }
        }
        edges
    }

    #[test]
    fn tile_preview_matches_a_full_mask_scan_after_crossing_stroke() {
        let mut editor = Editor::new();
        editor.load_buffer(PixelBuffer::new(200, 150, Color::WHITE), None);
        editor.settings.brush_size = 9;
        editor.selection_brush_begin((5, 5), false, false);
        // A zig-zag that crosses several 64px tiles.
        editor.selection_brush_to((150, 20));
        editor.selection_brush_to((150, 120));
        editor.selection_brush_to((20, 120));

        let width = editor.document.width() as i32;
        let height = editor.document.height() as i32;
        let mask = editor.sel_brush_mask.clone();
        let tiles = editor.selection_brush_tiles().expect("stroke is active");

        let mut boundary = Vec::new();
        let mut runs = Vec::new();
        for tile in tiles {
            boundary.extend(tile.boundary.iter().copied());
            runs.extend(tile.runs.iter().copied());
        }

        // The highlight runs must cover exactly the painted mask.
        let mut covered = vec![false; (width * height) as usize];
        for &(y, x0, x1) in &runs {
            for x in x0..=x1 {
                covered[(y * width + x) as usize] = true;
            }
        }
        assert_eq!(covered, mask);

        // The split tile outline must describe the same edges as one full scan.
        let full = rspaint_core::selection::boundary_segments(&mask, width as u32, height as u32);
        assert!(!boundary.is_empty());
        assert_eq!(unit_edges(&boundary), unit_edges(&full));
    }
}

use super::*;

pub(super) fn step_size(value: &mut u32, delta: i32, max: u32) {
    let next = (*value as i64 + delta as i64).clamp(1, max as i64);
    *value = next as u32;
}

pub(super) fn square_end(start: (i32, i32), p: (i32, i32)) -> (i32, i32) {
    let dx = p.0 - start.0;
    let dy = p.1 - start.1;
    let size = dx.abs().max(dy.abs());
    let sx = if dx < 0 { -1 } else { 1 };
    let sy = if dy < 0 { -1 } else { 1 };
    (start.0 + sx * size, start.1 + sy * size)
}

pub(super) fn snap_to_45(start: (i32, i32), p: (i32, i32)) -> (i32, i32) {
    let dx = (p.0 - start.0) as f64;
    let dy = (p.1 - start.1) as f64;
    if dx == 0.0 && dy == 0.0 {
        return start;
    }
    let eighth_turn = std::f64::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / eighth_turn).round() * eighth_turn;
    let distance = (dx * dx + dy * dy).sqrt();
    (
        start.0 + (angle.cos() * distance).round() as i32,
        start.1 + (angle.sin() * distance).round() as i32,
    )
}

pub(super) fn selection_from_region(region: NewRegion) -> Selection {
    let width = region.pixels.width();
    let height = region.pixels.height();
    let boundary = if region.mask {
        rspaint_core::selection::boundary_segments(&region.mask_bits, width, height)
    } else {
        Vec::new()
    };
    let runs = runs_from_mask(&region.mask_bits, width, height);
    Selection {
        pixels: region.pixels,
        x: region.x,
        y: region.y,
        outline: region.outline,
        mask: region.mask,
        boundary,
        mask_bits: region.mask_bits,
        runs,
        floating: false,
        dragging: false,
        moved: false,
        drag_offset: (0, 0),
        base: None,
        rotation_origin: None,
        rotation: 0.0,
        name: region.name,
    }
}

pub(super) fn mask_to_buffer(mask_bits: &[bool], width: u32, height: u32) -> PixelBuffer {
    let mut buffer = PixelBuffer::transparent(width, height);
    for y in 0..height {
        for x in 0..width {
            if mask_bits
                .get((y * width + x) as usize)
                .copied()
                .unwrap_or(false)
            {
                buffer.set(x, y, Color::WHITE);
            }
        }
    }
    buffer
}

pub(super) fn buffer_to_mask(buffer: &PixelBuffer) -> Vec<bool> {
    let (width, height) = (buffer.width(), buffer.height());
    let mut mask = vec![false; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            if buffer.get(x, y).is_some_and(|color| color.a > 127) {
                mask[(y * width + x) as usize] = true;
            }
        }
    }
    mask
}

pub(super) fn angle_from_up(pivot: (f32, f32), point: (f32, f32)) -> f32 {
    let dx = point.0 - pivot.0;
    let dy = point.1 - pivot.1;
    dx.atan2(-dy)
}

pub(super) fn rotate_content_around(
    pixels: &PixelBuffer,
    mask: &[bool],
    origin: (i32, i32),
    pivot: (f32, f32),
    degrees: f64,
) -> (PixelBuffer, Vec<bool>, (i32, i32)) {
    let (width, height) = (pixels.width() as f64, pixels.height() as f64);
    let (ox, oy) = (origin.0 as f64, origin.1 as f64);
    let (px, py) = (f64::from(pivot.0), f64::from(pivot.1));
    let radians = degrees.to_radians();
    let (cos, sin) = (radians.cos(), radians.sin());

    let corners = [
        (ox, oy),
        (ox + width, oy),
        (ox + width, oy + height),
        (ox, oy + height),
    ];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (cx, cy) in corners {
        let dx = cx - px;
        let dy = cy - py;
        let rx = px + dx * cos - dy * sin;
        let ry = py + dx * sin + dy * cos;
        min_x = min_x.min(rx);
        min_y = min_y.min(ry);
        max_x = max_x.max(rx);
        max_y = max_y.max(ry);
    }
    // Bound the output to avoid runaway allocations if the pivot is far.
    let max_extent = (width + height) * 2.0 + 256.0;
    let new_x = min_x.floor() as i32;
    let new_y = min_y.floor() as i32;
    let new_width = (max_x.ceil() - new_x as f64).clamp(1.0, max_extent) as u32;
    let new_height = (max_y.ceil() - new_y as f64).clamp(1.0, max_extent) as u32;
    let stride = new_width as usize;
    let mut out = PixelBuffer::transparent(new_width, new_height);
    let mut out_mask = vec![false; stride * new_height as usize];
    let source_w = pixels.width() as usize;
    for out_y in 0..new_height {
        for out_x in 0..new_width {
            let canvas_x = new_x as f64 + out_x as f64 + 0.5;
            let canvas_y = new_y as f64 + out_y as f64 + 0.5;
            let dx = canvas_x - px;
            let dy = canvas_y - py;
            let sx = px + dx * cos + dy * sin;
            let sy = py - dx * sin + dy * cos;
            let local_x = (sx - ox).floor();
            let local_y = (sy - oy).floor();
            if local_x >= 0.0 && local_y >= 0.0 && local_x < width && local_y < height {
                let (local_x, local_y) = (local_x as u32, local_y as u32);
                if mask
                    .get(local_y as usize * source_w + local_x as usize)
                    .copied()
                    .unwrap_or(false)
                {
                    if let Some(color) = pixels.get(local_x, local_y) {
                        out.set(out_x, out_y, color);
                        out_mask[out_y as usize * stride + out_x as usize] = true;
                    }
                }
            }
        }
    }

    // Crop to the tight bounding box of the rotated content so repeated
    // rotations don't accumulate empty margins.
    let mut crop_min_x = new_width;
    let mut crop_min_y = new_height;
    let mut crop_max_x = 0;
    let mut crop_max_y = 0;
    let mut any = false;
    for y in 0..new_height {
        for x in 0..new_width {
            if out_mask[y as usize * stride + x as usize] {
                any = true;
                crop_min_x = crop_min_x.min(x);
                crop_min_y = crop_min_y.min(y);
                crop_max_x = crop_max_x.max(x);
                crop_max_y = crop_max_y.max(y);
            }
        }
    }
    if !any {
        return (PixelBuffer::transparent(1, 1), vec![false], (new_x, new_y));
    }
    let crop_w = crop_max_x - crop_min_x + 1;
    let crop_h = crop_max_y - crop_min_y + 1;
    let crop_stride = crop_w as usize;
    let mut cropped = PixelBuffer::transparent(crop_w, crop_h);
    let mut cropped_mask = vec![false; crop_stride * crop_h as usize];
    for y in 0..crop_h {
        for x in 0..crop_w {
            let (sx, sy) = (crop_min_x + x, crop_min_y + y);
            if out_mask[sy as usize * stride + sx as usize] {
                cropped_mask[y as usize * crop_stride + x as usize] = true;
                if let Some(color) = out.get(sx, sy) {
                    cropped.set(x, y, color);
                }
            }
        }
    }
    (
        cropped,
        cropped_mask,
        (new_x + crop_min_x as i32, new_y + crop_min_y as i32),
    )
}

pub(super) fn runs_from_mask(mask_bits: &[bool], width: u32, height: u32) -> Vec<HighlightRun> {
    let mut runs = Vec::new();
    let w = width as i32;
    for y in 0..height as i32 {
        let mut x = 0;
        while x < w {
            let idx = (y as u32 * width + x as u32) as usize;
            if mask_bits.get(idx).copied().unwrap_or(false) {
                let start = x;
                while x < w
                    && mask_bits
                        .get((y as u32 * width + x as u32) as usize)
                        .copied()
                        .unwrap_or(false)
                {
                    x += 1;
                }
                runs.push((y, start, x - 1));
            } else {
                x += 1;
            }
        }
    }
    runs
}

pub(super) fn clear_region_masked(
    dst: &mut PixelBuffer,
    mask_bits: &[bool],
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    fill: Color,
) {
    for oy in 0..height as i32 {
        for ox in 0..width as i32 {
            if mask_bits
                .get((oy as u32 * width + ox as u32) as usize)
                .copied()
                .unwrap_or(false)
            {
                let dx = x + ox;
                let dy = y + oy;
                if dx >= 0 && dy >= 0 {
                    dst.set(dx as u32, dy as u32, fill);
                }
            }
        }
    }
}

pub(super) fn region_removes_any(previous: &Selection, region: &NewRegion) -> bool {
    let (px, py) = (previous.x, previous.y);
    let (pw, ph) = (previous.pixels.width(), previous.pixels.height());
    let rw = region.pixels.width();
    for oy in 0..region.pixels.height() as i32 {
        for ox in 0..rw as i32 {
            if !region
                .mask_bits
                .get((oy as u32 * rw + ox as u32) as usize)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            let lx = region.x + ox - px;
            let ly = region.y + oy - py;
            let selected = lx >= 0
                && ly >= 0
                && lx < pw as i32
                && ly < ph as i32
                && previous
                    .mask_bits
                    .get((ly as u32 * pw + lx as u32) as usize)
                    .copied()
                    .unwrap_or(true);
            if selected {
                return true;
            }
        }
    }
    false
}

pub(super) fn subtract_region(previous: Selection, region: &NewRegion) -> Option<Selection> {
    let (ax, ay) = (previous.x, previous.y);
    let (aw, ah) = (previous.pixels.width(), previous.pixels.height());
    let mut mask = if previous.mask_bits.is_empty() {
        vec![true; (aw * ah) as usize]
    } else {
        previous.mask_bits.clone()
    };
    let mut content = previous.pixels.clone();
    let rw = region.pixels.width();
    for oy in 0..region.pixels.height() as i32 {
        for ox in 0..rw as i32 {
            if !region
                .mask_bits
                .get((oy as u32 * rw + ox as u32) as usize)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            let lx = region.x + ox - ax;
            let ly = region.y + oy - ay;
            if lx >= 0 && ly >= 0 && lx < aw as i32 && ly < ah as i32 {
                mask[(ly as u32 * aw + lx as u32) as usize] = false;
                content.set(lx as u32, ly as u32, Color::TRANSPARENT);
            }
        }
    }
    if !mask.iter().any(|&selected| selected) {
        return None;
    }
    let boundary = rspaint_core::selection::boundary_segments(&mask, aw, ah);
    let runs = runs_from_mask(&mask, aw, ah);
    Some(Selection {
        pixels: content,
        x: ax,
        y: ay,
        outline: Vec::new(),
        mask: true,
        boundary,
        mask_bits: mask,
        runs,
        floating: previous.floating,
        dragging: false,
        moved: previous.moved,
        drag_offset: (0, 0),
        base: previous.base,
        rotation_origin: None,
        rotation: 0.0,
        name: previous.name,
    })
}

pub(super) fn region_adds_new(previous: &Selection, region: &NewRegion) -> bool {
    let (px, py) = (previous.x, previous.y);
    let (pw, ph) = (previous.pixels.width(), previous.pixels.height());
    let rw = region.pixels.width();
    for oy in 0..region.pixels.height() as i32 {
        for ox in 0..rw as i32 {
            if !region
                .mask_bits
                .get((oy as u32 * rw + ox as u32) as usize)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            let lx = region.x + ox - px;
            let ly = region.y + oy - py;
            let already = lx >= 0
                && ly >= 0
                && lx < pw as i32
                && ly < ph as i32
                && previous
                    .mask_bits
                    .get((ly as u32 * pw + lx as u32) as usize)
                    .copied()
                    .unwrap_or(true);
            if !already {
                return true;
            }
        }
    }
    false
}

pub(super) fn union_region(previous: Selection, region: NewRegion, fill: Color) -> Selection {
    let (ax, ay) = (previous.x, previous.y);
    let (aw, ah) = (previous.pixels.width(), previous.pixels.height());
    let (bx, by) = (region.x, region.y);
    let (bw, bh) = (region.pixels.width(), region.pixels.height());
    let min_x = ax.min(bx);
    let min_y = ay.min(by);
    let max_x = (ax + aw as i32).max(bx + bw as i32);
    let max_y = (ay + ah as i32).max(by + bh as i32);
    let width = (max_x - min_x) as u32;
    let height = (max_y - min_y) as u32;

    let mut merged = PixelBuffer::transparent(width, height);
    let mut merged_mask = vec![false; (width * height) as usize];

    for oy in 0..ah as i32 {
        for ox in 0..aw as i32 {
            let source = (oy as u32 * aw + ox as u32) as usize;
            if previous.mask_bits.get(source).copied().unwrap_or(true) {
                let dx = (ax - min_x + ox) as u32;
                let dy = (ay - min_y + oy) as u32;
                merged_mask[(dy * width + dx) as usize] = true;
                if let Some(color) = previous.pixels.get(ox as u32, oy as u32) {
                    if color.a > 0 {
                        merged.set(dx, dy, color);
                    }
                }
            }
        }
    }
    for oy in 0..bh as i32 {
        for ox in 0..bw as i32 {
            let source = (oy as u32 * bw + ox as u32) as usize;
            if region.mask_bits.get(source).copied().unwrap_or(false) {
                let dx = (bx - min_x + ox) as u32;
                let dy = (by - min_y + oy) as u32;
                merged_mask[(dy * width + dx) as usize] = true;
                if let Some(color) = region.pixels.get(ox as u32, oy as u32) {
                    if color.a > 0 {
                        merged.set(dx, dy, color);
                    }
                }
            }
        }
    }

    // If the previous selection was floating, keep it floating with an updated
    // base that also clears the newly added region.
    let floating = previous.floating;
    let base = if floating {
        previous.base.clone().map(|mut base| {
            clear_region_masked(&mut base, &region.mask_bits, bx, by, bw, bh, fill);
            base
        })
    } else {
        None
    };

    let boundary = rspaint_core::selection::boundary_segments(&merged_mask, width, height);
    let runs = runs_from_mask(&merged_mask, width, height);
    Selection {
        pixels: merged,
        x: min_x,
        y: min_y,
        outline: Vec::new(),
        mask: true,
        boundary,
        mask_bits: merged_mask,
        runs,
        floating,
        dragging: false,
        moved: previous.moved,
        drag_offset: (0, 0),
        base,
        rotation_origin: None,
        rotation: 0.0,
        name: region.name,
    }
}

/// Combines two selections into one, with the second taking priority where
/// their selected areas overlap.
pub(super) fn merge_selections(first: Selection, second: Selection) -> Selection {
    let (ax, ay) = (first.x, first.y);
    let (aw, ah) = (first.pixels.width(), first.pixels.height());
    let (bx, by) = (second.x, second.y);
    let (bw, bh) = (second.pixels.width(), second.pixels.height());
    let min_x = ax.min(bx);
    let min_y = ay.min(by);
    let max_x = (ax + aw as i32).max(bx + bw as i32);
    let max_y = (ay + ah as i32).max(by + bh as i32);
    let width = (max_x - min_x) as u32;
    let height = (max_y - min_y) as u32;

    let mut merged = PixelBuffer::transparent(width, height);
    let mut merged_mask = vec![false; (width * height) as usize];

    for (source_x, source_y, source_w, source_h, selection) in
        [(ax, ay, aw, ah, &first), (bx, by, bw, bh, &second)]
    {
        for oy in 0..source_h as i32 {
            for ox in 0..source_w as i32 {
                let index = (oy as u32 * source_w + ox as u32) as usize;
                if !selection.mask_bits.get(index).copied().unwrap_or(true) {
                    continue;
                }
                let dx = (source_x - min_x + ox) as u32;
                let dy = (source_y - min_y + oy) as u32;
                merged_mask[(dy * width + dx) as usize] = true;
                if let Some(color) = selection.pixels.get(ox as u32, oy as u32) {
                    if color.a > 0 {
                        merged.set(dx, dy, color);
                    }
                }
            }
        }
    }

    let is_rect = merged_mask.iter().all(|&selected| selected);
    let boundary = if is_rect {
        Vec::new()
    } else {
        rspaint_core::selection::boundary_segments(&merged_mask, width, height)
    };
    let runs = runs_from_mask(&merged_mask, width, height);
    Selection {
        pixels: merged,
        x: min_x,
        y: min_y,
        outline: Vec::new(),
        mask: !is_rect,
        boundary,
        mask_bits: merged_mask,
        runs,
        floating: first.floating || second.floating,
        dragging: false,
        moved: first.moved || second.moved,
        drag_offset: (0, 0),
        base: None,
        rotation_origin: None,
        rotation: 0.0,
        name: second.name,
    }
}

pub(super) fn union_rect(
    old_x: i32,
    old_y: i32,
    new_x: i32,
    new_y: i32,
    width: u32,
    height: u32,
) -> RectI {
    let last = (width as i32 - 1, height as i32 - 1);
    let old = RectI::between((old_x, old_y), (old_x + last.0, old_y + last.1));
    let new = RectI::between((new_x, new_y), (new_x + last.0, new_y + last.1));
    old.union(new)
}

pub(super) fn file_stem(path: &std::path::Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
}

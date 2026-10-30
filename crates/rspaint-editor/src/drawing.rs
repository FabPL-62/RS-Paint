use super::*;

impl Editor {
    pub fn clear_image(&mut self) {
        if !self.can_paint_active() {
            return;
        }
        let color = self.canvas_clear_color();
        self.document.buffer.fill(color);
        self.commit("Clear Image");
        self.mark_full();
    }
    pub fn set_tool(&mut self, tool: Tool) {
        if self.settings.tool == Tool::Polygon && tool != Tool::Polygon {
            self.finish_polygon();
        }
        if self.settings.tool == Tool::Curve && tool != Tool::Curve {
            self.curve_cancel();
        }
        if self.settings.tool == Tool::Text && tool != Tool::Text {
            self.commit_text();
        }
        if self.settings.tool == Tool::Magnifier && tool != Tool::Magnifier {
            self.zoom_rect = None;
        }
        if tool.is_drawing() {
            // Keep the selection so drawing stays clipped to it.
            self.settle_selection();
        } else if !tool.is_selection() {
            self.deselect();
        }
        // Diagonal brush shapes only apply to the Brush tool.
        if tool == Tool::SelectionBrush
            && matches!(
                self.settings.brush_shape,
                BrushShape::Diagonal | BrushShape::ReverseDiagonal
            )
        {
            self.settings.brush_shape = BrushShape::Circle;
        }
        self.settings.tool = tool;
    }

    pub fn polygon_points(&self) -> &[(i32, i32)] {
        &self.polygon_points
    }

    pub fn polygon_preview(&self) -> Option<(i32, i32)> {
        self.polygon_preview
    }

    pub fn set_polygon_preview(&mut self, preview: Option<(i32, i32)>) {
        self.polygon_preview = preview;
    }

    pub fn polygon_click(&mut self, p: (i32, i32)) {
        if self.polygon_points.len() >= 3 && self.polygon_near_start(p) {
            self.finish_polygon();
            return;
        }
        if self.polygon_points.is_empty() {
            self.polygon_stroke = self.colors.foreground;
            self.polygon_fill = self.colors.background;
        }
        self.polygon_points.push(p);
    }

    pub(super) fn polygon_near_start(&self, p: (i32, i32)) -> bool {
        self.polygon_points.first().is_some_and(|&(x, y)| {
            let dx = p.0 - x;
            let dy = p.1 - y;
            dx * dx + dy * dy <= 16
        })
    }

    pub fn finish_polygon(&mut self) {
        if !self.can_paint_active() {
            return;
        }
        let points = std::mem::take(&mut self.polygon_points);
        self.polygon_preview = None;
        if points.len() < 2 {
            return;
        }
        let stroke = self.polygon_stroke;
        let fill = self.polygon_fill;
        self.note_used_color(stroke);
        let brush = Brush::new(BrushShape::Square, self.settings.stroke_size)
            .with_blend(self.settings.paint_blend);
        let interior = match self.settings.fill_style {
            ShapeFill::Outline => None,
            ShapeFill::FilledWithOutline => Some(fill),
            ShapeFill::FilledOnly => Some(stroke),
        };
        if let Some(color) = interior {
            rspaint_core::draw::fill_polygon(
                &mut self.document.buffer,
                &points,
                color,
                self.settings.paint_blend,
            );
        }
        if self.settings.fill_style != ShapeFill::FilledOnly {
            rspaint_core::draw::draw_polyline(
                &mut self.document.buffer,
                &points,
                &brush,
                stroke,
                true,
            );
        }
        self.mark_full();
        self.commit("Polygon");
    }

    pub fn cancel_polygon(&mut self) {
        self.polygon_points.clear();
        self.polygon_preview = None;
    }

    pub fn curve_pointer_down(&mut self, p: (i32, i32)) {
        if !self.can_paint_active() {
            return;
        }
        if self.curve_points.is_empty() {
            self.curve_base = Some(self.document.buffer.clone());
            self.curve_points.push(p);
            self.curve_points.push(p);
        } else {
            self.curve_points.push(p);
        }
        self.curve_dragging = true;
        self.curve_redraw();
    }

    pub fn curve_pointer_move(&mut self, p: (i32, i32)) {
        if !self.curve_dragging {
            return;
        }
        if let Some(last) = self.curve_points.last_mut() {
            *last = p;
        }
        self.curve_redraw();
    }

    pub fn curve_pointer_up(&mut self) {
        self.curve_dragging = false;
        if self.curve_points.len() >= 4 {
            self.curve_base = None;
            self.curve_points.clear();
            self.note_used_color(self.colors.foreground);
            self.commit("Curve");
        }
    }

    pub fn curve_cancel(&mut self) {
        if self.curve_points.is_empty() {
            return;
        }
        if let Some(base) = self.curve_base.take() {
            self.document.buffer = base;
            self.mark_full();
        }
        self.curve_points.clear();
        self.curve_dragging = false;
    }

    pub(super) fn curve_redraw(&mut self) {
        let Some(base) = self.curve_base.as_ref() else {
            return;
        };
        let (width, height) = (self.document.width(), self.document.height());
        self.document
            .buffer
            .copy_region_from(base, 0, 0, width, height);
        let brush = Brush::new(BrushShape::Square, self.settings.stroke_size)
            .with_blend(self.settings.paint_blend);
        let color = self.colors.foreground;
        let points = self.curve_points.clone();
        let buffer = &mut self.document.buffer;
        match points.as_slice() {
            [] => {}
            [p] => brush.stamp(buffer, p.0, p.1, color),
            [start, end] => draw_line(buffer, *start, *end, &brush, color),
            [start, end, control] => {
                draw_quadratic_curve(buffer, *start, *control, *end, &brush, color)
            }
            [start, end, control_1, control_2] => {
                draw_bezier(buffer, *start, *control_1, *control_2, *end, &brush, color)
            }
            _ => {}
        }
        if let Some(base) = self.curve_base.clone() {
            let rect = RectI {
                x0: 0,
                y0: 0,
                x1: width as i32 - 1,
                y1: height as i32 - 1,
            };
            self.clip_region_to_selection(&base, rect);
        }
        self.mark_full();
    }

    pub fn transform_selection_direct(&mut self, choice: FlipRotate) {
        if self.selection.is_none() {
            return;
        }
        let angle = 0.0;
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
        self.transform_selection(apply, name);
    }

    pub(super) fn tool_radius(&self) -> i32 {
        match self.settings.tool {
            Tool::Pencil => (self.settings.pencil_size / 2) as i32,
            Tool::Brush => (self.settings.brush_size / 2) as i32 + 1,
            Tool::Eraser => (self.settings.eraser_size / 2) as i32 + 1,
            Tool::Airbrush => self.settings.airbrush_size as i32 + 1,
            _ => self.settings.stroke_size as i32 + 1,
        }
    }

    pub(super) fn active_color(&self, button: PointerButton) -> Color {
        match button {
            PointerButton::Secondary => self.colors.background,
            _ => self.colors.foreground,
        }
    }

    pub(super) fn canvas_clear_color(&self) -> Color {
        // Clear Image fills with the secondary color (its alpha is honored).
        self.colors.background
    }

    pub(super) fn eraser_color(&self) -> Color {
        // The eraser paints the secondary color, so its alpha (opaque, partial
        // or fully transparent) is always honored.
        self.colors.background
    }

    pub fn begin_stroke(&mut self, p: (i32, i32), button: PointerButton) {
        if !self.can_paint_active() {
            return;
        }
        let color = self.active_color(button);
        self.note_used_color(color);
        // The fill color is the opposite of the stroke color (right-drag swaps).
        let fill_color = if button == PointerButton::Secondary {
            self.colors.foreground
        } else {
            self.colors.background
        };
        let tool = self.settings.tool;
        if tool.is_shape() {
            let preview_rect = RectI::between(p, p).expand(self.settings.stroke_size as i32 + 1);
            self.stroke = Some(Stroke {
                color,
                fill_color,
                start: p,
                last: p,
                name: tool.name(),
                color_eraser: false,
                base: Some(self.document.buffer.clone()),
                preview_rect: Some(preview_rect),
            });
            self.draw_shape(p, p, color, fill_color);
            if let Some(base) = self.stroke.as_ref().and_then(|stroke| stroke.base.clone()) {
                self.clip_region_to_selection(&base, preview_rect);
            }
            self.mark_rect(preview_rect);
        } else if tool.is_freehand() {
            let color_eraser = tool == Tool::Eraser && button == PointerButton::Secondary;
            self.stroke = Some(Stroke {
                color,
                fill_color,
                start: p,
                last: p,
                name: if color_eraser {
                    "Color Eraser"
                } else {
                    tool.name()
                },
                color_eraser,
                base: Some(self.document.buffer.clone()),
                preview_rect: None,
            });
            // Fresh per-stroke mask so a single pass paints each pixel once.
            // When clipping to a selection, mark unselected pixels as painted
            // so the stroke never touches them.
            let count = (self.document.width() * self.document.height()) as usize;
            let clip_mask = if self.clipping_to_selection() {
                self.selection_canvas_mask()
            } else {
                None
            };
            self.stroke_painted = match clip_mask {
                Some(mask) => mask.into_iter().map(|selected| !selected).collect(),
                None => vec![false; count],
            };
            self.thin_prev2 = None;
            self.thin_prev1 = None;
            self.thin_prev1_new = false;
            self.smooth_position = Some((p.0 as f32, p.1 as f32));
            self.smooth_target = Some(p);
            self.draw_freehand(p, p, color);
            self.mark_segment(p, p, self.tool_radius());
        }
    }

    pub fn update_stroke(&mut self, p: (i32, i32), shift: bool) {
        let Some((last, start, color)) = self.stroke.as_ref().map(|s| (s.last, s.start, s.color))
        else {
            return;
        };
        if self.settings.tool.is_shape() {
            let end = self.shape_end(start, p, shift);
            self.redraw_shape(end);
            if let Some(stroke) = self.stroke.as_mut() {
                stroke.last = p;
            }
            return;
        }
        let target = self.smoothed_target(p);
        if target != last {
            self.draw_freehand(last, target, color);
            self.mark_segment(last, target, self.tool_radius());
            if let Some(stroke) = self.stroke.as_mut() {
                stroke.last = target;
            }
        }
    }

    /// Ends the stroke at the pointer's release position, so smoothing catches
    /// up to the exact point where the user let go.
    pub fn end_stroke_at(&mut self, p: (i32, i32), shift: bool) {
        self.update_stroke(p, shift);
        self.end_stroke(shift);
    }

    pub fn end_stroke(&mut self, shift: bool) {
        self.drain_smoothing();
        // Redraw the final shape in case Shift was toggled after the last move.
        if self.settings.tool.is_shape() {
            if let Some((start, last)) = self.stroke.as_ref().map(|s| (s.start, s.last)) {
                let end = self.shape_end(start, last, shift);
                self.redraw_shape(end);
            }
        }
        if let Some(stroke) = self.stroke.take() {
            self.commit(stroke.name);
        }
    }

    /// Whether pointer smoothing applies to the active tool.
    fn smoothing_active(&self) -> bool {
        self.settings.stroke_smoothing > 0
            && matches!(self.settings.tool, Tool::Pencil | Tool::Brush)
    }

    /// Filter strength: 1 means no smoothing, smaller values lag more.
    fn smoothing_alpha(&self) -> f32 {
        let amount = self.settings.stroke_smoothing.min(100) as f32 / 100.0;
        (1.0 - 0.95 * amount).clamp(0.02, 1.0)
    }

    /// Advances the smoothing filter and returns the integer point to draw.
    fn smoothed_target(&mut self, p: (i32, i32)) -> (i32, i32) {
        if !self.smoothing_active() {
            self.smooth_position = None;
            self.smooth_target = None;
            return p;
        }
        let current = self.smooth_position.unwrap_or((p.0 as f32, p.1 as f32));
        let alpha = self.smoothing_alpha();
        let next = (
            current.0 + (p.0 as f32 - current.0) * alpha,
            current.1 + (p.1 as f32 - current.1) * alpha,
        );
        self.smooth_position = Some(next);
        self.smooth_target = Some(p);
        (next.0.round() as i32, next.1.round() as i32)
    }

    /// Catches the smoothed pointer up to the last raw position before the
    /// stroke is committed, drawing the remaining path instead of jumping.
    fn drain_smoothing(&mut self) {
        if !self.smoothing_active() {
            self.smooth_position = None;
            self.smooth_target = None;
            return;
        }
        let (Some(target), Some(mut current), Some((mut last, color))) = (
            self.smooth_target,
            self.smooth_position,
            self.stroke.as_ref().map(|s| (s.last, s.color)),
        ) else {
            self.smooth_position = None;
            self.smooth_target = None;
            return;
        };
        // A stronger filter during the drain converges quickly without a
        // visible straight jump at the end.
        let alpha = self.smoothing_alpha().max(0.2);
        for _ in 0..64 {
            current.0 += (target.0 as f32 - current.0) * alpha;
            current.1 += (target.1 as f32 - current.1) * alpha;
            let next = (current.0.round() as i32, current.1.round() as i32);
            if next != last {
                self.draw_freehand(last, next, color);
                self.mark_segment(last, next, self.tool_radius());
                last = next;
                if let Some(stroke) = self.stroke.as_mut() {
                    stroke.last = next;
                }
            }
            if (current.0 - target.0 as f32).abs() < 0.5
                && (current.1 - target.1 as f32).abs() < 0.5
            {
                break;
            }
        }
        if last != target {
            self.draw_freehand(last, target, color);
            self.mark_segment(last, target, self.tool_radius());
            if let Some(stroke) = self.stroke.as_mut() {
                stroke.last = target;
            }
        }
        self.smooth_position = None;
        self.smooth_target = None;
    }

    pub fn has_stroke(&self) -> bool {
        self.stroke.is_some()
    }

    pub fn cancel_stroke(&mut self) {
        let Some(stroke) = self.stroke.take() else {
            return;
        };
        if let Some(base) = stroke.base {
            self.document.buffer = base;
        }
        self.stroke_painted.clear();
        self.smooth_position = None;
        self.smooth_target = None;
        self.mark_full();
    }

    pub(super) fn shape_end(&self, start: (i32, i32), p: (i32, i32), shift: bool) -> (i32, i32) {
        if !shift {
            return p;
        }
        match self.settings.tool {
            Tool::Line => snap_to_45(start, p),
            Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle => square_end(start, p),
            _ => p,
        }
    }

    pub(super) fn redraw_shape(&mut self, end: (i32, i32)) {
        let Some((start, color, fill)) = self
            .stroke
            .as_ref()
            .map(|s| (s.start, s.color, s.fill_color))
        else {
            return;
        };
        let new_rect = RectI::between(start, end).expand(self.settings.stroke_size as i32 + 1);
        let previous = self.stroke.as_ref().and_then(|s| s.preview_rect);
        let restore = match previous {
            Some(previous) => previous.union(new_rect),
            None => new_rect,
        };
        if let Some((x, y, w, h)) = restore.clamped(self.document.width(), self.document.height()) {
            if let Some(base) = self.stroke.as_ref().and_then(|s| s.base.as_ref()) {
                self.document.buffer.copy_region_from(base, x, y, w, h);
            }
        }
        self.draw_shape(start, end, color, fill);
        if let Some(base) = self.stroke.as_ref().and_then(|stroke| stroke.base.clone()) {
            self.clip_region_to_selection(&base, new_rect);
        }
        if let Some(stroke) = self.stroke.as_mut() {
            stroke.preview_rect = Some(new_rect);
        }
        self.mark_rect(restore);
    }

    pub(super) fn draw_freehand(&mut self, from: (i32, i32), to: (i32, i32), color: Color) {
        match self.settings.tool {
            Tool::Pencil => {
                if self.settings.pencil_size == 1 && self.settings.pixel_perfect {
                    self.draw_thin_freehand(from, to, color);
                    return;
                }
                let brush = Brush::new(BrushShape::Square, self.settings.pencil_size)
                    .with_blend(self.settings.paint_blend);
                let width = self.document.width();
                draw_line_masked(
                    &mut self.document.buffer,
                    from,
                    to,
                    &brush,
                    color,
                    &mut self.stroke_painted,
                    width,
                );
            }
            Tool::Brush => {
                if self.settings.brush_size == 1 && self.settings.pixel_perfect {
                    self.draw_thin_freehand(from, to, color);
                    return;
                }
                let brush = Brush::new(self.settings.brush_shape, self.settings.brush_size)
                    .with_blend(self.settings.paint_blend);
                let width = self.document.width();
                draw_line_masked(
                    &mut self.document.buffer,
                    from,
                    to,
                    &brush,
                    color,
                    &mut self.stroke_painted,
                    width,
                );
            }
            Tool::Eraser => {
                // Erasing always overwrites (including alpha), so a transparent
                // erase color actually clears pixels instead of being a no-op.
                let brush = Brush::new(self.settings.eraser_shape, self.settings.eraser_size)
                    .with_blend(BlendMode::Replace);
                if self
                    .stroke
                    .as_ref()
                    .is_some_and(|stroke| stroke.color_eraser)
                {
                    // Color Eraser replaces the foreground color with the
                    // background color (even in transparent mode).
                    let erase_to = self.colors.background;
                    let erase_from = self.colors.foreground;
                    erase_line(
                        &mut self.document.buffer,
                        from,
                        to,
                        &brush,
                        erase_from,
                        erase_to,
                    );
                } else {
                    let color = self.eraser_color();
                    self.note_transparency(color);
                    let width = self.document.width();
                    draw_line_masked(
                        &mut self.document.buffer,
                        from,
                        to,
                        &brush,
                        color,
                        &mut self.stroke_painted,
                        width,
                    );
                }
            }
            Tool::Airbrush => self.spray(to, color),
            _ => {}
        }
    }

    /// Paints one segment of a 1 px freehand stroke, removing the middle
    /// pixel of every L corner (pixel-perfect mode).
    ///
    /// Corner removal only looks at the last two kept pixels and the new one.
    /// A pixel that was already painted before this step (a self-intersection
    /// or a clipped pixel) is never removed, so crossings keep their pixels.
    pub(super) fn draw_thin_freehand(&mut self, from: (i32, i32), to: (i32, i32), color: Color) {
        let blend = self.settings.paint_blend;
        let width = self.document.width() as i32;
        let height = self.document.height() as i32;
        let stride = width as usize;
        let Some(base) = self.stroke.as_ref().and_then(|stroke| stroke.base.as_ref()) else {
            return;
        };
        let buffer = &mut self.document.buffer;
        let painted = &mut self.stroke_painted;
        let mut prev2 = self.thin_prev2;
        let mut prev1 = self.thin_prev1;
        let mut prev1_new = self.thin_prev1_new;
        let mut first = true;
        walk_line(from, to, |x, y| {
            // The line starts where the previous segment ended; that pixel
            // was already processed.
            if first {
                first = false;
                if Some((x, y)) == prev1 {
                    return;
                }
            }
            let mut painted_now = false;
            if x >= 0 && y >= 0 && x < width && y < height {
                let index = y as usize * stride + x as usize;
                if index < painted.len() && !painted[index] {
                    painted[index] = true;
                    buffer.paint(x as u32, y as u32, color, blend);
                    painted_now = true;
                }
            }
            // An L corner: `prev1` sits between two diagonal neighbors.
            // Remove it if this stroke painted it, leaving the two diagonal
            // pixels connected and the stroke one pixel wide.
            if prev1_new {
                if let (Some(a), Some(b)) = (prev2, prev1) {
                    if is_corner(a, b, (x, y)) && unpaint_thin(buffer, base, painted, stride, b) {
                        prev1 = Some((x, y));
                        prev1_new = painted_now;
                        return;
                    }
                }
            }
            prev2 = prev1;
            prev1 = Some((x, y));
            prev1_new = painted_now;
        });
        self.thin_prev2 = prev2;
        self.thin_prev1 = prev1;
        self.thin_prev1_new = prev1_new;
    }

    pub(super) fn spray(&mut self, at: (i32, i32), color: Color) {
        let radius = self.settings.airbrush_size.max(1) as i32;
        let density = (radius * radius) as u32;
        for _ in 0..density {
            let span = (2 * radius + 1) as u32;
            let dx = (self.rng.next_u32() % span) as i32 - radius;
            let dy = (self.rng.next_u32() % span) as i32 - radius;
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let x = at.0 + dx;
            let y = at.1 + dy;
            if x >= 0 && y >= 0 {
                self.document
                    .buffer
                    .paint(x as u32, y as u32, color, self.settings.paint_blend);
            }
        }
    }

    pub(super) fn draw_shape(&mut self, a: (i32, i32), b: (i32, i32), color: Color, fill: Color) {
        let brush = Brush::new(BrushShape::Square, self.settings.stroke_size)
            .with_blend(self.settings.paint_blend);
        let fill_style = self.settings.fill_style;
        // Whether to fill the interior, and with which color.
        let interior = match fill_style {
            ShapeFill::Outline => None,
            ShapeFill::FilledWithOutline => Some(fill),
            ShapeFill::FilledOnly => Some(color),
        };
        let outline = fill_style != ShapeFill::FilledOnly;
        match self.settings.tool {
            Tool::Line => draw_line(&mut self.document.buffer, a, b, &brush, color),
            Tool::Rectangle => {
                if let Some(interior) = interior {
                    fill_rectangle(
                        &mut self.document.buffer,
                        a,
                        b,
                        interior,
                        self.settings.paint_blend,
                    );
                }
                if outline {
                    draw_rectangle(&mut self.document.buffer, a, b, &brush, color);
                }
            }
            Tool::Ellipse => {
                if let Some(interior) = interior {
                    fill_ellipse(
                        &mut self.document.buffer,
                        a,
                        b,
                        interior,
                        self.settings.paint_blend,
                    );
                }
                if outline {
                    draw_ellipse(&mut self.document.buffer, a, b, &brush, color);
                }
            }
            Tool::RoundedRectangle => {
                let radius = self.settings.round_radius as i32;
                if let Some(interior) = interior {
                    fill_rounded_rectangle(
                        &mut self.document.buffer,
                        a,
                        b,
                        radius,
                        interior,
                        self.settings.paint_blend,
                    );
                }
                if outline {
                    draw_rounded_rectangle(&mut self.document.buffer, a, b, radius, &brush, color);
                }
            }
            _ => {}
        }
    }

    pub fn click_tool(&mut self, p: (i32, i32), button: PointerButton, shift: bool) {
        if self.settings.tool == Tool::Fill && !self.can_paint_active() {
            return;
        }
        match self.settings.tool {
            Tool::Fill => {
                let color = self.active_color(button);
                self.note_used_color(color);
                let base = self
                    .clipping_to_selection()
                    .then(|| self.document.buffer.clone());
                flood_fill(
                    &mut self.document.buffer,
                    p.0,
                    p.1,
                    color,
                    !shift,
                    self.settings.paint_blend,
                );
                if let Some(base) = base {
                    let rect = RectI {
                        x0: 0,
                        y0: 0,
                        x1: self.document.width() as i32 - 1,
                        y1: self.document.height() as i32 - 1,
                    };
                    self.clip_region_to_selection(&base, rect);
                }
                self.commit("Fill With Color");
                self.mark_full();
            }
            Tool::PickColor => {
                if p.0 >= 0 && p.1 >= 0 {
                    if let Some(color) = self.document.flattened_pixel(p.0, p.1) {
                        if button == PointerButton::Secondary {
                            self.colors.background = color;
                        } else {
                            self.colors.foreground = color;
                        }
                    }
                }
            }
            Tool::Magnifier => {
                if button == PointerButton::Secondary {
                    self.zoom_out();
                } else {
                    self.zoom_in();
                }
            }
            _ => {}
        }
    }

    pub fn adjust_round_radius(&mut self, delta: i32) {
        let next = (self.settings.round_radius as i32 + delta).clamp(0, MAX_ROUND_RADIUS as i32);
        self.settings.round_radius = next as u32;
    }

    pub fn adjust_tool_size(&mut self, delta: i32) {
        match self.settings.tool {
            Tool::Pencil => step_size(&mut self.settings.pencil_size, delta, 50),
            Tool::Brush | Tool::SelectionBrush => {
                step_size(&mut self.settings.brush_size, delta, 500)
            }
            Tool::Eraser => step_size(&mut self.settings.eraser_size, delta, 500),
            Tool::Airbrush => step_size(&mut self.settings.airbrush_size, delta, 500),
            Tool::Line
            | Tool::Curve
            | Tool::Polygon
            | Tool::Rectangle
            | Tool::Ellipse
            | Tool::RoundedRectangle => {
                step_size(&mut self.settings.stroke_size, delta, 500);
            }
            _ => {}
        }
    }

    pub fn toggle_selection_transparency(&mut self) {
        if self.selection.is_none() {
            return;
        }
        self.settings.selection_transparent = !self.settings.selection_transparent;
        self.restamp_selection();
    }

    pub(super) fn restamp_selection(&mut self) {
        let background = if self.settings.selection_transparent {
            Some(self.colors.background)
        } else {
            None
        };
        let mut dirty = None;
        if let Some(selection) = self.selection.as_mut() {
            if selection.floating {
                let (x, y) = (selection.x, selection.y);
                let rect = RectI::between(
                    (x, y),
                    (
                        x + selection.pixels.width() as i32 - 1,
                        y + selection.pixels.height() as i32 - 1,
                    ),
                );
                if let Some((cx, cy, cw, ch)) =
                    rect.clamped(self.document.width(), self.document.height())
                {
                    if let Some(base) = selection.base.as_ref() {
                        self.document.buffer.copy_region_from(base, cx, cy, cw, ch);
                    }
                }
                rspaint_core::selection::stamp(
                    &mut self.document.buffer,
                    &selection.pixels,
                    x,
                    y,
                    background,
                );
                dirty = Some(rect);
            }
        }
        if let Some(rect) = dirty {
            self.mark_rect(rect);
        }
    }
}

/// Whether `b` is the corner of an L made by `a` and `c`.
///
/// True when `a` and `c` are diagonal neighbors and `b` is one of the two
/// orthogonal pixels between them. Removing `b` then leaves `a` and `c`
/// connected by their shared corner.
fn is_corner(a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> bool {
    let diagonal = (a.0 - c.0).abs() == 1 && (a.1 - c.1).abs() == 1;
    let next_to_a = (b.0 - a.0).abs() + (b.1 - a.1).abs() == 1;
    let next_to_c = (b.0 - c.0).abs() + (b.1 - c.1).abs() == 1;
    diagonal && next_to_a && next_to_c
}

/// Restores a pixel removed by pixel-perfect corner correction.
///
/// Only pixels painted by the current stroke are restored: the pre-stroke
/// `base` value is put back and the pixel is marked unpainted so a later part
/// of the stroke may paint it again. Returns whether the pixel was restored.
fn unpaint_thin(
    buffer: &mut PixelBuffer,
    base: &PixelBuffer,
    painted: &mut [bool],
    stride: usize,
    p: (i32, i32),
) -> bool {
    let (x, y) = p;
    if x < 0 || y < 0 || x as u32 >= buffer.width() || y as u32 >= buffer.height() {
        return false;
    }
    let index = y as usize * stride + x as usize;
    if index >= painted.len() || !painted[index] {
        return false;
    }
    let (x, y) = (x as u32, y as u32);
    let Some(original) = base.get(x, y) else {
        return false;
    };
    if buffer.get(x, y) == Some(original) {
        // Already equal to the pre-stroke image: not painted by this stroke.
        return false;
    }
    buffer.set(x, y, original);
    painted[index] = false;
    true
}

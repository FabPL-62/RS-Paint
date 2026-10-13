use super::*;

impl Editor {
    pub fn clear_image(&mut self) {
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
            self.polygon_stroke = self.settings.foreground;
            self.polygon_fill = self.settings.background;
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
            self.note_used_color(self.settings.foreground);
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
        let color = self.settings.foreground;
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
            PointerButton::Secondary => self.settings.background,
            _ => self.settings.foreground,
        }
    }

    pub(super) fn canvas_clear_color(&self) -> Color {
        // Clear Image fills with the secondary color (its alpha is honored).
        self.settings.background
    }

    pub(super) fn eraser_color(&self) -> Color {
        // The eraser paints the secondary color, so its alpha (opaque, partial
        // or fully transparent) is always honored.
        self.settings.background
    }

    pub fn begin_stroke(&mut self, p: (i32, i32), button: PointerButton) {
        let color = self.active_color(button);
        self.note_used_color(color);
        // The fill color is the opposite of the stroke color (right-drag swaps).
        let fill_color = if button == PointerButton::Secondary {
            self.settings.foreground
        } else {
            self.settings.background
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
        } else {
            self.draw_freehand(last, p, color);
            self.mark_segment(last, p, self.tool_radius());
        }
        if let Some(stroke) = self.stroke.as_mut() {
            stroke.last = p;
        }
    }

    pub fn end_stroke(&mut self, shift: bool) {
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
                    let erase_to = self.settings.background;
                    let erase_from = self.settings.foreground;
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
                    if let Some(color) = self.document.buffer.get(p.0 as u32, p.1 as u32) {
                        if button == PointerButton::Secondary {
                            self.settings.background = color;
                        } else {
                            self.settings.foreground = color;
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
            Some(self.settings.background)
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

use super::*;

impl Editor {
    pub fn capture_shared(&self) -> Shared {
        self.settings.clone()
    }

    pub fn apply_shared(&mut self, shared: &Shared) {
        self.settings = shared.clone();
    }

    pub fn is_modified(&self) -> bool {
        self.document.modified
    }

    pub fn invalidate_canvas(&mut self) {
        self.mark_full();
    }

    pub fn cursor_size(&self) -> u32 {
        // A temporary color-pick uses a single-pixel cursor.
        if self.pick_hold.is_some() {
            return 1;
        }
        match self.settings.tool {
            Tool::Pencil => self.settings.pencil_size,
            Tool::Brush | Tool::SelectionBrush => self.settings.brush_size,
            Tool::Eraser => self.settings.eraser_size,
            // The airbrush sprays within `airbrush_size` pixels of the cursor.
            Tool::Airbrush => self.settings.airbrush_size * 2 + 1,
            Tool::Line | Tool::Curve | Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle => {
                self.settings.stroke_size
            }
            _ => 1,
        }
    }

    pub fn cursor_shape_code(&self) -> f32 {
        if self.pick_hold.is_some() {
            return 0.0;
        }
        let code = match self.settings.tool {
            Tool::Airbrush => 1,
            Tool::Brush | Tool::SelectionBrush => match self.settings.brush_shape {
                BrushShape::Square => 0,
                BrushShape::Circle => 1,
                BrushShape::Diagonal => 2,
                BrushShape::ReverseDiagonal => 3,
            },
            Tool::Eraser => match self.settings.eraser_shape {
                BrushShape::Circle => 1,
                _ => 0,
            },
            _ => 0,
        };
        code as f32
    }

    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.settings.foreground, &mut self.settings.background);
    }

    pub fn note_transparency(&mut self, color: Color) {
        if color.a < 255 {
            self.document.transparent = true;
        }
    }

    pub fn note_used_color(&mut self, color: Color) {
        self.note_transparency(color);
        let count = self.settings.custom_colors.len();
        if count == 0 {
            return;
        }
        if self.settings.palette.contains(&color) || self.settings.custom_colors.contains(&color) {
            return;
        }
        self.settings.custom_colors[self.custom_index % count] = color;
        self.custom_index = (self.custom_index + 1) % count;
    }

    pub fn reset_colors(&mut self) {
        self.settings.palette = palette::default_palette();
    }

    pub fn capture_colors(&mut self) {
        let count = rspaint_core::palette::CUSTOM_COLOR_COUNT;
        let mut found: Vec<Color> = Vec::new();
        'scan: for y in 0..self.document.height() {
            for x in 0..self.document.width() {
                let Some(color) = self.document.buffer.get(x, y) else {
                    continue;
                };
                if color.a == 0 || self.settings.palette.contains(&color) || found.contains(&color)
                {
                    continue;
                }
                found.push(color);
                if found.len() >= count {
                    break 'scan;
                }
            }
        }
        let mut row = vec![Color::BLACK; count];
        for (slot, color) in row.iter_mut().zip(&found) {
            *slot = *color;
        }
        self.settings.custom_colors = row;
        self.custom_index = found.len() % count;
    }

    pub fn pick_hold(&self) -> Option<PickTarget> {
        self.pick_hold
    }

    pub fn pick_color_at(&mut self, p: (i32, i32), target: PickTarget) {
        if p.0 < 0 || p.1 < 0 {
            return;
        }
        if let Some(color) = self.document.buffer.get(p.0 as u32, p.1 as u32) {
            match target {
                PickTarget::Primary => self.settings.foreground = color,
                PickTarget::Secondary => self.settings.background = color,
            }
        }
    }
}

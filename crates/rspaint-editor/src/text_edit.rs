use super::*;

impl Editor {
    pub fn has_text_box(&self) -> bool {
        self.text_box.is_some()
    }

    pub fn text_box_rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.text_box.map(|b| (b.x, b.y, b.width, b.height))
    }

    pub fn text_type(&mut self, text: &str) {
        if self.text_box.is_some() {
            self.text_buffer.push_str(text);
        }
    }

    pub fn text_backspace(&mut self) {
        if self.text_box.is_some() {
            self.text_buffer.pop();
        }
    }

    pub fn text_newline(&mut self) {
        if self.text_box.is_some() {
            self.text_buffer.push('\n');
        }
    }

    pub fn text_caret_segment(&self) -> Option<((f32, f32), (f32, f32))> {
        let text_box = self.text_box?;
        let style = crate::text::TextStyle {
            family: text_box.family,
            size: text_box.size,
            bold: text_box.bold,
            italic: text_box.italic,
            underline: text_box.underline,
        };
        if text_box.vertical {
            let (x, top, height) =
                crate::text::caret(&self.fonts, style, text_box.height, &self.text_buffer);
            let x0 = (text_box.width - 1 - top - height) as f32;
            let x1 = (text_box.width - 1 - top) as f32;
            let y = x as f32;
            Some(((x0, y), (x1, y)))
        } else {
            let (x, top, height) =
                crate::text::caret(&self.fonts, style, text_box.width, &self.text_buffer);
            let cx = x as f32;
            Some(((cx, top as f32), (cx, (top + height) as f32)))
        }
    }

    pub fn text_creating_rect(&self) -> Option<((i32, i32), (i32, i32))> {
        self.text_creating
    }

    pub fn begin_text_box(&mut self, p: (i32, i32)) {
        self.text_creating = Some((p, p));
    }

    pub fn update_text_box(&mut self, p: (i32, i32)) {
        if let Some((_, end)) = self.text_creating.as_mut() {
            *end = p;
        }
    }

    pub fn finish_text_box(&mut self) {
        if let Some((a, b)) = self.text_creating.take() {
            let width = (a.0 - b.0).abs() + 1;
            let height = (a.1 - b.1).abs() + 1;
            if width > 1 && height > 1 {
                self.open_text_box(a.0.min(b.0), a.1.min(b.1), width, height);
            }
        }
    }

    pub fn default_text_box(&mut self, p: (i32, i32)) {
        let style = self.text_style();
        let height = crate::text::line_height(&self.fonts, style) + 8;
        self.open_text_box(p.0, p.1, 120, height);
    }

    pub(super) fn open_text_box(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.text_buffer.clear();
        self.text_base = Some(self.document.buffer.clone());
        self.text_box = Some(TextBox {
            x,
            y,
            width,
            height,
            family: self.settings.text_family,
            size: self.settings.text_size,
            bold: self.settings.text_bold,
            italic: self.settings.text_italic,
            underline: self.settings.text_underline,
            vertical: self.settings.text_vertical,
            transparent: self.settings.text_transparent,
        });
    }

    pub(super) fn text_style(&self) -> crate::text::TextStyle {
        crate::text::TextStyle {
            family: self.settings.text_family,
            size: self.settings.text_size,
            bold: self.settings.text_bold,
            italic: self.settings.text_italic,
            underline: self.settings.text_underline,
        }
    }

    pub fn render_text_preview(&mut self) {
        let Some(text_box) = self.text_box else {
            return;
        };
        if let Some(base) = self.text_base.as_ref() {
            let (width, height) = (self.document.width(), self.document.height());
            self.document
                .buffer
                .copy_region_from(base, 0, 0, width, height);
        }
        if !self.text_buffer.trim().is_empty() {
            let text = self.text_buffer.clone();
            self.draw_text_box(text_box, &text);
        }
        self.mark_full();
    }

    pub(super) fn draw_text_box(&mut self, text_box: TextBox, text: &str) {
        if !text_box.transparent {
            fill_rectangle(
                &mut self.document.buffer,
                (text_box.x, text_box.y),
                (
                    text_box.x + text_box.width - 1,
                    text_box.y + text_box.height - 1,
                ),
                self.settings.background,
                BlendMode::Replace,
            );
        }
        let style = crate::text::TextStyle {
            family: text_box.family,
            size: text_box.size,
            bold: text_box.bold,
            italic: text_box.italic,
            underline: text_box.underline,
        };
        let width = text_box.width.max(1) as u32;
        let height = text_box.height.max(1) as u32;
        if text_box.vertical {
            // Vertical text: lay it out along the box height, then rotate 90°.
            let mut temp = PixelBuffer::transparent(height, width);
            crate::text::draw_text(
                &self.fonts,
                &mut temp,
                (0, 0),
                text_box.height,
                text,
                style,
                self.settings.foreground,
            );
            let rotated = crate::text::rotate_90_cw(&temp);
            crate::text::composite(&mut self.document.buffer, &rotated, text_box.x, text_box.y);
        } else {
            let mut temp = PixelBuffer::transparent(width, height);
            crate::text::draw_text(
                &self.fonts,
                &mut temp,
                (0, 0),
                text_box.width,
                text,
                style,
                self.settings.foreground,
            );
            crate::text::composite(&mut self.document.buffer, &temp, text_box.x, text_box.y);
        }
    }

    pub fn commit_text(&mut self) {
        let Some(text_box) = self.text_box.take() else {
            return;
        };
        let text = std::mem::take(&mut self.text_buffer);
        if let Some(base) = self.text_base.take() {
            let (width, height) = (self.document.width(), self.document.height());
            self.document
                .buffer
                .copy_region_from(&base, 0, 0, width, height);
        }
        if !text.trim().is_empty() {
            self.draw_text_box(text_box, &text);
            self.note_used_color(self.settings.foreground);
            self.mark_full();
            self.commit("Text");
        } else {
            self.mark_full();
        }
    }

    pub fn cancel_text(&mut self) {
        if let Some(base) = self.text_base.take() {
            let (width, height) = (self.document.width(), self.document.height());
            self.document
                .buffer
                .copy_region_from(&base, 0, 0, width, height);
        }
        self.text_box = None;
        self.text_buffer.clear();
        self.text_creating = None;
        self.mark_full();
    }
}

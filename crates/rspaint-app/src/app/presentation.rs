use super::*;

impl PaintApp {
    pub fn stage_canvas_upload(&mut self) -> crate::canvas_renderer::PendingUpload {
        self.canvas_upload.stage(&mut self.editor);
        self.canvas_upload.clone()
    }

    pub(super) fn update_title(&mut self, ctx: &egui::Context) {
        let marker = if self.editor.document.modified {
            " *"
        } else {
            ""
        };
        let title = format!("{}{} - RS Paint", self.document_name(ctx), marker);
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    pub fn paint_cursor(&self, painter: &egui::Painter, pointer: egui::Pos2) {
        self.cursors
            .for_state(self.editor.settings.tool, self.editor.pick_hold)
            .paint(painter, pointer);
    }

    pub fn paint_pivot_cross(&self, painter: &egui::Painter, center: egui::Pos2, opacity: f32) {
        // Nudge the image slightly right so its visual center sits on the pivot.
        let center = center + egui::vec2(1.0, 0.0);
        self.cursors.paint_precise_dotted(painter, center, opacity);
    }

    pub fn canvas_uniforms(
        &self,
        magnification: f32,
        pixels_per_point: f32,
        uv_rect: [f32; 4],
    ) -> Uniforms {
        let magnification_px = magnification * pixels_per_point;
        let (cursor, show_cursor) = match self.editor.pointer {
            Some((cx, cy))
                if cx >= 0
                    && cy >= 0
                    && cx < self.editor.document.width() as i32
                    && cy < self.editor.document.height() as i32 =>
            {
                let size = self.editor.cursor_size().max(1);
                let offset = (size - 1) / 2;
                let origin_x = (cx - offset as i32) as f32;
                let origin_y = (cy - offset as i32) as f32;
                let shape = self.editor.cursor_shape_code();
                ([origin_x, origin_y, size as f32, shape], true)
            }
            _ => ([0.0; 4], false),
        };
        Uniforms {
            canvas_mag: [
                self.editor.document.width() as f32,
                self.editor.document.height() as f32,
                magnification_px,
                8.0 * pixels_per_point,
            ],
            cursor,
            flags: [
                bool_f32(self.editor.document.transparent),
                bool_f32(self.editor.settings.show_grid && magnification >= 8.0),
                bool_f32(show_cursor),
                bool_f32(self.srgb_output),
            ],
            uv_rect,
            params: [
                self.editor.settings.grid_opacity as f32 / 100.0,
                0.0,
                0.0,
                0.0,
            ],
        }
    }

    pub fn update_editor_top(&mut self, ctx: &egui::Context) {
        self.handle_shortcuts(ctx);
        self.update_pick_hold(ctx);
        self.update_title(ctx);

        ui::menubar::show(self, ctx);
        ui::toolbox::show(self, ctx);
    }

    pub fn update_editor_bottom(&mut self, ctx: &egui::Context) {
        ui::status_bar::show(self, ctx);
        ui::colorbox::show(self, ctx);
        ui::color_picker::show(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui::canvas_view::show(self, ui);
        });

        if self.show_about {
            egui::Window::new(crate::i18n::tr(ctx, "About RS Paint"))
                .id(egui::Id::new("dialog-about-rs-paint"))
                .collapsible(false)
                .resizable(false)
                .open(&mut self.show_about)
                .show(ctx, |ui| {
                    ui.label("RS Paint");
                    ui.label(crate::i18n::tr(ui.ctx(), "A Rust port of JS Paint."));
                    ui.label(crate::i18n::tr(ui.ctx(), "Licensed under the MIT license."));
                });
        }

        self.show_attributes_window(ctx);
        self.show_history_window(ctx);
        self.show_flip_rotate_window(ctx);
        self.show_stretch_skew_window(ctx);
        self.show_custom_zoom_window(ctx);
        self.show_thumbnail_window(ctx);
        self.show_error_window(ctx);
        self.show_bitmap_view(ctx);
    }
}

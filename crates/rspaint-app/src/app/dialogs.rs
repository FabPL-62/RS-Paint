use super::*;

impl PaintApp {
    fn scaled_rgba(&self, max_w: f32, max_h: f32) -> ([usize; 2], Vec<u8>) {
        let w = self.editor.document.width().max(1);
        let h = self.editor.document.height().max(1);
        let scale = (max_w / w as f32).min(max_h / h as f32);
        let tw = ((w as f32 * scale).round().max(1.0) as u32).max(1);
        let th = ((h as f32 * scale).round().max(1.0) as u32).max(1);
        let mut bytes = Vec::with_capacity((tw * th * 4) as usize);
        for y in 0..th {
            let sy = (((y as f32 + 0.5) / scale) as u32).min(h - 1);
            for x in 0..tw {
                let sx = (((x as f32 + 0.5) / scale) as u32).min(w - 1);
                let c = self
                    .editor
                    .document
                    .composite
                    .get(sx, sy)
                    .unwrap_or(Color::TRANSPARENT);
                bytes.extend_from_slice(&[c.r, c.g, c.b, c.a]);
            }
        }
        ([tw as usize, th as usize], bytes)
    }
    pub(super) fn show_attributes_window(&mut self, ctx: &egui::Context) {
        if !self.show_attributes {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(crate::i18n::tr(ctx, "Attributes"))
            .id(egui::Id::new("dialog-attributes"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                egui::Grid::new("attributes-grid").show(ui, |ui| {
                    ui.label(crate::i18n::tr(ui.ctx(), "Width:"));
                    ui.add(egui::DragValue::new(&mut self.attributes_width).range(1..=10_000));
                    ui.end_row();
                    ui.label(crate::i18n::tr(ui.ctx(), "Height:"));
                    ui.add(egui::DragValue::new(&mut self.attributes_height).range(1..=10_000));
                    ui.end_row();
                });
                ui.checkbox(
                    &mut self.attributes_transparent,
                    crate::i18n::tr(ui.ctx(), "Transparent"),
                );
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr(ui.ctx(), "OK")).clicked() {
                        apply = true;
                    }
                    if ui.button(crate::i18n::tr(ui.ctx(), "Cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.apply_attributes();
        } else if cancel || !open {
            self.show_attributes = false;
        }
    }

    pub(super) fn show_history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history {
            return;
        }
        let nodes: Vec<(rspaint_core::NodeId, usize, String)> = self
            .editor
            .history
            .nodes()
            .iter()
            .map(|node| {
                (
                    node.id,
                    history_depth(&self.editor.history, node.id),
                    node.name.clone(),
                )
            })
            .collect();
        let current = self.editor.history.current();
        let mut open = true;
        let mut jump = None;
        egui::Window::new(crate::i18n::tr(ctx, "History"))
            .id(egui::Id::new("dialog-history"))
            .default_size([240.0, 320.0])
            .open(&mut open)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (id, depth, name) in &nodes {
                        let label =
                            format!("{}{}", "    ".repeat(*depth), crate::i18n::tr(ctx, name));
                        if ui.selectable_label(*id == current, label).clicked() {
                            jump = Some(*id);
                        }
                    }
                });
            });
        if let Some(id) = jump {
            self.editor.jump_to_history(id);
        }
        if !open {
            self.show_history = false;
        }
    }

    pub(super) fn show_flip_rotate_window(&mut self, ctx: &egui::Context) {
        if !self.show_flip_rotate {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(crate::i18n::tr(ctx, "Flip and Rotate"))
            .id(egui::Id::new("dialog-flip-and-rotate"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.radio_value(
                    &mut self.flip_rotate,
                    FlipRotate::FlipHorizontal,
                    crate::i18n::tr(ui.ctx(), "Flip horizontal"),
                );
                ui.radio_value(
                    &mut self.flip_rotate,
                    FlipRotate::FlipVertical,
                    crate::i18n::tr(ui.ctx(), "Flip vertical"),
                );
                ui.radio_value(
                    &mut self.flip_rotate,
                    FlipRotate::Rotate90,
                    crate::i18n::tr(ui.ctx(), "Rotate 90°"),
                );
                ui.radio_value(
                    &mut self.flip_rotate,
                    FlipRotate::Rotate180,
                    crate::i18n::tr(ui.ctx(), "Rotate 180°"),
                );
                ui.radio_value(
                    &mut self.flip_rotate,
                    FlipRotate::Rotate270,
                    crate::i18n::tr(ui.ctx(), "Rotate 270°"),
                );
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.flip_rotate,
                        FlipRotate::Arbitrary,
                        crate::i18n::tr(ui.ctx(), "Angle:"),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.flip_rotate_angle)
                            .range(-360.0..=360.0)
                            .suffix("°"),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr(ui.ctx(), "OK")).clicked() {
                        apply = true;
                    }
                    if ui.button(crate::i18n::tr(ui.ctx(), "Cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.apply_flip_rotate();
        } else if cancel || !open {
            self.show_flip_rotate = false;
        }
    }

    pub(super) fn show_custom_zoom_window(&mut self, ctx: &egui::Context) {
        if !self.show_custom_zoom {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        let current = self.editor.magnification * 100.0;
        egui::Window::new(crate::i18n::tr(ctx, "Custom Zoom"))
            .id(egui::Id::new("dialog-custom-zoom"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(crate::i18n::message(
                    ctx,
                    "Current zoom: {current}%",
                    &[("current", &current.to_string())],
                ));
                for pct in [100.0_f32, 200.0, 400.0, 600.0, 800.0] {
                    let selected = (self.custom_zoom_percent - pct).abs() < 0.5;
                    if ui.radio(selected, format!("{pct}%")).clicked() {
                        self.custom_zoom_percent = pct;
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr(ui.ctx(), "Custom:"));
                    ui.add(
                        egui::DragValue::new(&mut self.custom_zoom_percent)
                            .range(10.0..=1000.0)
                            .suffix("%"),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr(ui.ctx(), "OK")).clicked() {
                        apply = true;
                    }
                    if ui.button(crate::i18n::tr(ui.ctx(), "Cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.editor
                .set_magnification(self.custom_zoom_percent / 100.0);
            self.show_custom_zoom = false;
        } else if cancel || !open {
            self.show_custom_zoom = false;
        }
    }

    pub(super) fn show_thumbnail_window(&mut self, ctx: &egui::Context) {
        if !self.show_thumbnail {
            self.thumbnail = None;
            return;
        }
        if self.thumbnail_revision != self.editor.revision() || self.thumbnail.is_none() {
            self.editor.document.composite_all();
            let (size, bytes) = self.scaled_rgba(108.0, 92.0);
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &bytes);
            match &mut self.thumbnail {
                Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
                None => {
                    self.thumbnail =
                        Some(ctx.load_texture("thumbnail", image, egui::TextureOptions::NEAREST));
                }
            }
            self.thumbnail_revision = self.editor.revision();
        }
        let mut open = true;
        egui::Window::new(crate::i18n::tr(ctx, "Thumbnail"))
            .id(egui::Id::new("dialog-thumbnail"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                if let Some(texture) = &self.thumbnail {
                    let size = texture.size_vec2();
                    ui.add(egui::Image::new((texture.id(), size)));
                }
            });
        if !open {
            self.show_thumbnail = false;
        }
    }

    pub(super) fn show_bitmap_view(&mut self, ctx: &egui::Context) {
        if !self.show_bitmap {
            return;
        }
        let exit = ctx.input(|i| {
            i.key_pressed(Key::Escape) || (i.modifiers.command && i.key_pressed(Key::F))
        });
        let screen = ctx.viewport_rect();
        let pixels_per_point = ctx.pixels_per_point();
        let doc = egui::vec2(
            self.editor.document.width().max(1) as f32,
            self.editor.document.height().max(1) as f32,
        );
        let fit = (screen.width() / doc.x).min(screen.height() / doc.y);
        let size = doc * fit;
        let image_rect = egui::Rect::from_center_size(screen.center(), size);
        let mut clicked = false;
        egui::Area::new("bitmap-view".into())
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (bg, response) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(bg, 0.0, egui::Color32::from_gray(80));
                clicked = response.clicked();
                if self.has_gpu() && size.x > 0.0 && size.y > 0.0 {
                    let mut uniforms =
                        self.canvas_uniforms(fit, pixels_per_point, [0.0, 0.0, 1.0, 1.0]);
                    uniforms.flags[1] = 0.0; // no pixel grid
                    uniforms.flags[2] = 0.0; // no tool cursor
                    let upload = self.stage_canvas_upload();
                    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                        image_rect,
                        CanvasCallback {
                            window_id: self.window_id(),
                            uniforms,
                            upload,
                        },
                    ));
                }
            });
        if exit || clicked {
            self.show_bitmap = false;
        }
    }

    pub(super) fn show_stretch_skew_window(&mut self, ctx: &egui::Context) {
        if !self.show_stretch_skew {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(crate::i18n::tr(ctx, "Stretch and Skew"))
            .id(egui::Id::new("dialog-stretch-and-skew"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "Stretch")).strong());
                percent_row(
                    ui,
                    crate::i18n::tr(ui.ctx(), "Horizontal:"),
                    &mut self.stretch_x,
                );
                percent_row(
                    ui,
                    crate::i18n::tr(ui.ctx(), "Vertical:"),
                    &mut self.stretch_y,
                );
                ui.add_space(4.0);
                ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "Skew")).strong());
                angle_row(
                    ui,
                    crate::i18n::tr(ui.ctx(), "Horizontal:"),
                    &mut self.skew_h,
                );
                angle_row(ui, crate::i18n::tr(ui.ctx(), "Vertical:"), &mut self.skew_v);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr(ui.ctx(), "OK")).clicked() {
                        apply = true;
                    }
                    if ui.button(crate::i18n::tr(ui.ctx(), "Cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.apply_stretch_skew();
        } else if cancel || !open {
            self.show_stretch_skew = false;
        }
    }

    pub(super) fn show_error_window(&mut self, ctx: &egui::Context) {
        let Some(message) = self.error_message.clone() else {
            return;
        };
        let mut dismiss = false;
        egui::Window::new(crate::i18n::tr(ctx, "Error"))
            .id(egui::Id::new("dialog-error"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(message);
                if ui.button(crate::i18n::tr(ui.ctx(), "OK")).clicked() {
                    dismiss = true;
                }
            });
        if dismiss {
            self.error_message = None;
        }
    }
}

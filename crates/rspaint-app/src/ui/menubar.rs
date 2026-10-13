//! The top menu bar, mirroring JS Paint's menus.

use eframe::egui;

use crate::app::PaintApp;

/// Draws the menu bar and applies any selected action.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu-bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(crate::i18n::tr(ui.ctx(), "File"), |ui| {
                if ui.button(crate::i18n::tr(ui.ctx(), "New")).clicked() {
                    app.editor.reset_document();
                    ui.close();
                }
                if ui.button(crate::i18n::tr(ui.ctx(), "Open...")).clicked() {
                    app.open_file_dialog(ctx);
                    ui.close();
                }
                if ui.button(crate::i18n::tr(ui.ctx(), "Save")).clicked() {
                    app.save(ctx);
                    ui.close();
                }
                if ui.button(crate::i18n::tr(ui.ctx(), "Save As...")).clicked() {
                    app.save_as(ctx);
                    ui.close();
                }
                ui.separator();
                if ui.button(crate::i18n::tr(ui.ctx(), "Exit")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            ui.menu_button(crate::i18n::tr(ui.ctx(), "Edit"), |ui| {
                let undo_label = match app.editor.history.undo_name() {
                    Some(name) => crate::i18n::message(
                        ui.ctx(),
                        "Undo {name}",
                        &[("name", crate::i18n::tr(ui.ctx(), name))],
                    ),
                    None => String::from("Undo"),
                };
                if ui
                    .add_enabled(app.editor.history.can_undo(), egui::Button::new(undo_label))
                    .clicked()
                {
                    app.editor.undo();
                    ui.close();
                }
                let redo_label = match app.editor.history.redo_name() {
                    Some(name) => crate::i18n::message(
                        ui.ctx(),
                        "Redo {name}",
                        &[("name", crate::i18n::tr(ui.ctx(), name))],
                    ),
                    None => String::from("Redo"),
                };
                if ui
                    .add_enabled(app.editor.history.can_redo(), egui::Button::new(redo_label))
                    .clicked()
                {
                    app.editor.redo();
                    ui.close();
                }
                ui.separator();
                let has_selection = app.editor.selection.is_some();
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new(crate::i18n::tr(ui.ctx(), "Cut")),
                    )
                    .clicked()
                {
                    app.cut_selection();
                    ui.close();
                }
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new(crate::i18n::tr(ui.ctx(), "Copy")),
                    )
                    .clicked()
                {
                    app.copy_selection();
                    ui.close();
                }
                if ui.button(crate::i18n::tr(ui.ctx(), "Paste")).clicked() {
                    app.paste_clipboard();
                    ui.close();
                }
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "Paste From..."))
                    .clicked()
                {
                    app.paste_from_file(ctx);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new(crate::i18n::tr(ui.ctx(), "Copy To...")),
                    )
                    .clicked()
                {
                    app.copy_to_file(ctx);
                    ui.close();
                }
                ui.separator();
                if ui.button(crate::i18n::tr(ui.ctx(), "History")).clicked() {
                    app.show_history = true;
                    ui.close();
                }
            });

            ui.menu_button(crate::i18n::tr(ui.ctx(), "View"), |ui| {
                ui.menu_button(crate::i18n::tr(ui.ctx(), "Zoom"), |ui| {
                    if ui.button(crate::i18n::tr(ui.ctx(), "Zoom In")).clicked() {
                        app.editor.zoom_in();
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr(ui.ctx(), "Zoom Out")).clicked() {
                        app.editor.zoom_out();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            app.editor.magnification != 1.0,
                            egui::Button::new(crate::i18n::tr(ui.ctx(), "Normal Size")),
                        )
                        .clicked()
                    {
                        app.editor.set_magnification(1.0);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            app.editor.magnification != 4.0,
                            egui::Button::new(crate::i18n::tr(ui.ctx(), "Large Size")),
                        )
                        .clicked()
                    {
                        app.editor.set_magnification(4.0);
                        ui.close();
                    }
                    if ui
                        .button(crate::i18n::tr(ui.ctx(), "Zoom To Window"))
                        .clicked()
                    {
                        app.editor.zoom_to_window();
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr(ui.ctx(), "Custom...")).clicked() {
                        app.open_custom_zoom_dialog();
                        ui.close();
                    }
                });
                ui.separator();
                let mut show_grid = app.editor.settings.show_grid;
                if ui
                    .add_enabled(
                        app.editor.magnification >= 8.0,
                        egui::Checkbox::new(&mut show_grid, crate::i18n::tr(ui.ctx(), "Show Grid")),
                    )
                    .changed()
                {
                    app.editor.toggle_grid();
                }
                let mut show_thumbnail = app.show_thumbnail;
                if ui
                    .checkbox(
                        &mut show_thumbnail,
                        crate::i18n::tr(ui.ctx(), "Show Thumbnail"),
                    )
                    .changed()
                {
                    app.show_thumbnail = show_thumbnail;
                }
                ui.separator();
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "View Bitmap"))
                    .clicked()
                {
                    app.show_bitmap = true;
                    ui.close();
                }
            });

            ui.menu_button(crate::i18n::tr(ui.ctx(), "Image"), |ui| {
                let mut opaque = !app.editor.settings.selection_transparent;
                if ui
                    .checkbox(&mut opaque, crate::i18n::tr(ui.ctx(), "Draw Opaque"))
                    .changed()
                {
                    app.editor.toggle_selection_transparency();
                }
                ui.separator();
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "Attributes..."))
                    .clicked()
                {
                    app.open_attributes_dialog();
                    ui.close();
                }
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "Clear Image"))
                    .clicked()
                {
                    app.editor.clear_image();
                    ui.close();
                }
                ui.separator();
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "Flip/Rotate..."))
                    .clicked()
                {
                    app.open_flip_rotate_dialog();
                    ui.close();
                }
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "Invert Colors"))
                    .clicked()
                {
                    app.editor.invert_colors();
                    ui.close();
                }
                let can_crop = app.editor.selection.is_some();
                if ui
                    .add_enabled(
                        can_crop,
                        egui::Button::new(crate::i18n::tr(ui.ctx(), "Crop to Selection")),
                    )
                    .clicked()
                {
                    app.editor.crop_to_selection();
                    ui.close();
                }
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "Stretch/Skew..."))
                    .clicked()
                {
                    app.open_stretch_skew_dialog();
                    ui.close();
                }
            });

            ui.menu_button(crate::i18n::tr(ui.ctx(), "Colors"), |ui| {
                ui.add_enabled(
                    false,
                    egui::Button::new(crate::i18n::tr(ui.ctx(), "Edit Colors")),
                );
            });

            ui.menu_button(crate::i18n::tr(ui.ctx(), "Help"), |ui| {
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "About RS Paint"))
                    .clicked()
                {
                    app.show_about = true;
                    ui.close();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(crate::i18n::tr(ui.ctx(), "Language"), |ui| {
                    let current = crate::i18n::locale(ui.ctx());
                    for language in crate::i18n::Locale::ALL {
                        if ui
                            .selectable_label(current == language, language.name())
                            .clicked()
                        {
                            crate::i18n::set_locale(ui.ctx(), language);
                            ui.close();
                        }
                    }
                });
            });
        });
    });
}

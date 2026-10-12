//! The top menu bar, mirroring JS Paint's menus.

use eframe::egui;

use crate::app::PaintApp;

/// Draws the menu bar and applies any selected action.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu-bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    app.reset_document();
                    ui.close();
                }
                if ui.button("Open...").clicked() {
                    app.open_file_dialog();
                    ui.close();
                }
                if ui.button("Save").clicked() {
                    app.save();
                    ui.close();
                }
                if ui.button("Save As...").clicked() {
                    app.save_as();
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            ui.menu_button("Edit", |ui| {
                let undo_label = match app.history.undo_name() {
                    Some(name) => format!("Undo {name}"),
                    None => String::from("Undo"),
                };
                if ui
                    .add_enabled(app.history.can_undo(), egui::Button::new(undo_label))
                    .clicked()
                {
                    app.undo();
                    ui.close();
                }
                let redo_label = match app.history.redo_name() {
                    Some(name) => format!("Redo {name}"),
                    None => String::from("Redo"),
                };
                if ui
                    .add_enabled(app.history.can_redo(), egui::Button::new(redo_label))
                    .clicked()
                {
                    app.redo();
                    ui.close();
                }
                ui.separator();
                let has_selection = app.selection.is_some();
                if ui
                    .add_enabled(has_selection, egui::Button::new("Cut"))
                    .clicked()
                {
                    app.cut_selection();
                    ui.close();
                }
                if ui
                    .add_enabled(has_selection, egui::Button::new("Copy"))
                    .clicked()
                {
                    app.copy_selection();
                    ui.close();
                }
                if ui.button("Paste").clicked() {
                    app.paste_clipboard();
                    ui.close();
                }
                if ui.button("Paste From...").clicked() {
                    app.paste_from_file();
                    ui.close();
                }
                if ui
                    .add_enabled(has_selection, egui::Button::new("Copy To..."))
                    .clicked()
                {
                    app.copy_to_file();
                    ui.close();
                }
                ui.separator();
                if ui.button("History").clicked() {
                    app.show_history = true;
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                ui.menu_button("Zoom", |ui| {
                    if ui.button("Zoom In").clicked() {
                        app.zoom_in();
                        ui.close();
                    }
                    if ui.button("Zoom Out").clicked() {
                        app.zoom_out();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(app.magnification != 1.0, egui::Button::new("Normal Size"))
                        .clicked()
                    {
                        app.set_magnification(1.0);
                        ui.close();
                    }
                    if ui
                        .add_enabled(app.magnification != 4.0, egui::Button::new("Large Size"))
                        .clicked()
                    {
                        app.set_magnification(4.0);
                        ui.close();
                    }
                    if ui.button("Zoom To Window").clicked() {
                        app.zoom_to_window();
                        ui.close();
                    }
                    if ui.button("Custom...").clicked() {
                        app.open_custom_zoom_dialog();
                        ui.close();
                    }
                });
                ui.separator();
                let mut show_grid = app.show_grid;
                if ui
                    .add_enabled(
                        app.magnification >= 8.0,
                        egui::Checkbox::new(&mut show_grid, "Show Grid"),
                    )
                    .changed()
                {
                    app.toggle_grid();
                }
                let mut show_thumbnail = app.show_thumbnail;
                if ui.checkbox(&mut show_thumbnail, "Show Thumbnail").changed() {
                    app.show_thumbnail = show_thumbnail;
                }
                ui.separator();
                if ui.button("View Bitmap").clicked() {
                    app.show_bitmap = true;
                    ui.close();
                }
            });

            ui.menu_button("Image", |ui| {
                let mut opaque = !app.selection_transparent;
                if ui.checkbox(&mut opaque, "Draw Opaque").changed() {
                    app.selection_transparent = !opaque;
                }
                ui.separator();
                if ui.button("Attributes...").clicked() {
                    app.open_attributes_dialog();
                    ui.close();
                }
                if ui.button("Clear Image").clicked() {
                    app.clear_image();
                    ui.close();
                }
                ui.separator();
                if ui.button("Flip/Rotate...").clicked() {
                    app.open_flip_rotate_dialog();
                    ui.close();
                }
                if ui.button("Invert Colors").clicked() {
                    app.invert_colors();
                    ui.close();
                }
                let can_crop = app.selection.is_some();
                if ui
                    .add_enabled(can_crop, egui::Button::new("Crop to Selection"))
                    .clicked()
                {
                    app.crop_to_selection();
                    ui.close();
                }
                if ui.button("Stretch/Skew...").clicked() {
                    app.open_stretch_skew_dialog();
                    ui.close();
                }
            });

            ui.menu_button("Colors", |ui| {
                ui.add_enabled(false, egui::Button::new("Edit Colors"));
            });

            ui.menu_button("Help", |ui| {
                if ui.button("About RS Paint").clicked() {
                    app.show_about = true;
                    ui.close();
                }
            });
        });
    });
}

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
                if ui.button("History").clicked() {
                    app.show_history = true;
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui.button("Zoom In").clicked() {
                    app.zoom_in();
                    ui.close();
                }
                if ui.button("Zoom Out").clicked() {
                    app.zoom_out();
                    ui.close();
                }
                if ui.button("Reset Zoom").clicked() {
                    app.magnification = 1.0;
                    ui.close();
                }
                if ui.button("Adjust Zoom").clicked() {
                    app.adjust_zoom();
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

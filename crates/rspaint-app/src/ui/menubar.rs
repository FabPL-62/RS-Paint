//! Application menus with hover navigation after the first click.
use crate::app::PaintApp;
use eframe::egui;

const ROOT_MENUS: [&str; 7] = [
    "File", "Edit", "View", "Image", "Colors", "Help", "Language",
];

fn popup_id(ctx: &egui::Context, key: &str) -> egui::Id {
    egui::Id::new(("main-menu", ctx.viewport_id(), key))
}

fn root_button(ui: &mut egui::Ui, key: &str) -> egui::Response {
    let selected = egui::Popup::is_id_open(ui.ctx(), popup_id(ui.ctx(), key));
    ui.add(egui::Button::new(crate::i18n::tr(ui.ctx(), key)).selected(selected))
}

pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu-bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            let was_open = ROOT_MENUS
                .iter()
                .any(|key| egui::Popup::is_id_open(ctx, popup_id(ctx, key)));
            let mut buttons = Vec::new();
            for key in &ROOT_MENUS[..6] {
                buttons.push((*key, root_button(ui, key)));
            }
            let language = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    root_button(ui, "Language")
                })
                .inner;
            buttons.push(("Language", language));

            // Resolve the active section before painting any popup, so only one
            // set of options is visible in this frame.
            if let Some((key, _)) = buttons.iter().find(|(_, response)| response.clicked()) {
                egui::Popup::toggle_id(ctx, popup_id(ctx, key));
                ctx.request_repaint();
            } else if was_open {
                if let Some((key, _)) = buttons.iter().find(|(_, response)| response.hovered()) {
                    let id = popup_id(ctx, key);
                    if !egui::Popup::is_id_open(ctx, id) {
                        egui::Popup::open_id(ctx, id);
                        ctx.request_repaint();
                    }
                }
            }
            for (key, response) in &buttons {
                egui::Popup::menu(response)
                    .id(popup_id(ctx, key))
                    .open_memory(None)
                    .show(|ui| match *key {
                        "File" => file_menu(app, ctx, ui),
                        "Edit" => edit_menu(app, ctx, ui),
                        "View" => view_menu(app, ui),
                        "Image" => image_menu(app, ui),
                        "Colors" => colors_menu(ui),
                        "Help" => help_menu(app, ui),
                        "Language" => language_menu(ui),
                        _ => unreachable!("known root menu"),
                    });
            }
        });
    });
}

fn file_menu(app: &mut PaintApp, ctx: &egui::Context, ui: &mut egui::Ui) {
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
}

fn edit_menu(app: &mut PaintApp, ctx: &egui::Context, ui: &mut egui::Ui) {
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
}

fn view_menu(app: &mut PaintApp, ui: &mut egui::Ui) {
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
}

fn image_menu(app: &mut PaintApp, ui: &mut egui::Ui) {
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
}

fn colors_menu(ui: &mut egui::Ui) {
    ui.add_enabled(
        false,
        egui::Button::new(crate::i18n::tr(ui.ctx(), "Edit Colors")),
    );
}

fn help_menu(app: &mut PaintApp, ui: &mut egui::Ui) {
    if ui
        .button(crate::i18n::tr(ui.ctx(), "About RS Paint"))
        .clicked()
    {
        app.show_about = true;
        ui.close();
    }
}

fn language_menu(ui: &mut egui::Ui) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        app: &mut PaintApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| show(app, ctx),
        )
    }

    fn label_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("menu label is rendered")
    }

    fn has_label(output: &egui::FullOutput, label: &str) -> bool {
        output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label))
    }

    #[test]
    fn open_menu_switches_on_hover_in_both_directions_without_extra_clicks() {
        let ctx = egui::Context::default();
        crate::i18n::set_locale(&ctx, crate::i18n::Locale::English);
        let mut app = PaintApp::new_blank(true, false, &ctx);
        let output = frame(&ctx, &mut app, Vec::new());
        let file = label_center(&output, "File");
        let edit = label_center(&output, "Edit");
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(edit)]);
        assert!(
            !egui::Popup::is_any_open(&ctx),
            "hover alone must not open a closed menu bar"
        );
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(file),
                    egui::Event::PointerButton {
                        pos: file,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(egui::Popup::is_id_open(&ctx, popup_id(&ctx, "File")));
        let output = frame(&ctx, &mut app, vec![egui::Event::PointerMoved(edit)]);
        assert!(egui::Popup::is_id_open(&ctx, popup_id(&ctx, "Edit")));
        assert!(!egui::Popup::is_id_open(&ctx, popup_id(&ctx, "File")));
        assert!(!has_label(&output, "Open..."));
        // Newly opened egui popups have an initial invisible sizing pass.
        let output = frame(&ctx, &mut app, Vec::new());
        assert!(has_label(&output, "Paste"));
        let output = frame(&ctx, &mut app, vec![egui::Event::PointerMoved(file)]);
        assert!(egui::Popup::is_id_open(&ctx, popup_id(&ctx, "File")));
        assert!(!egui::Popup::is_id_open(&ctx, popup_id(&ctx, "Edit")));
        assert!(!has_label(&output, "Paste"));
        let output = frame(&ctx, &mut app, Vec::new());
        assert!(has_label(&output, "Open..."));
    }
}

//! The status bar along the very bottom.

use eframe::egui;

use crate::app::{PaintApp, SelectionOutline};

/// Draws pointer coordinates, canvas size, zoom level and selection size.
pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status-bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let coords = match app.editor.pointer {
                Some((x, y)) => format!("{x}, {y}"),
                None => String::new(),
            };
            ui.label(coords);
            ui.separator();
            ui.label(format!(
                "{} x {}",
                app.editor.document.width(),
                app.editor.document.height()
            ));
            ui.separator();
            ui.label(format!("{}%", (app.editor.magnification * 100.0).round()));
            if let Some((width, height)) = selection_size(app, ctx) {
                ui.separator();
                ui.label(format!(
                    "{}: {width} x {height}",
                    crate::i18n::tr(ctx, "Selection")
                ));
            }
        });
    });
}

fn selection_size(app: &PaintApp, ctx: &egui::Context) -> Option<(u32, u32)> {
    if let Some(SelectionOutline::Rect { width, height, .. }) = app.editor.creating_outline() {
        return Some((width, height));
    }
    let show_transform = ctx.input(|input| input.modifiers.command)
        && !app.editor.clipping_to_selection()
        && app.editor.settings.tool.is_selection();
    if show_transform {
        return app
            .editor
            .selection_bounds()
            .map(|(_, _, width, height)| (width, height));
    }
    app.editor
        .pointer
        .and_then(|pointer| app.editor.selection_portion_size(pointer))
}

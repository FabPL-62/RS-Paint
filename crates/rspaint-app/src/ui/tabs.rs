//! Tab appearance; document and window actions belong to the workspace.
use crate::{i18n::tr, theme};
use eframe::egui;

pub const TAB_HEIGHT: f32 = 36.0;
const TAB_MAX_WIDTH: f32 = 220.0;

pub fn tab_widget(ui: &mut egui::Ui, label: &str, selected: bool) -> (egui::Response, bool) {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let pad = 12.0;
    let close_size = 22.0;
    let mut job = egui::text::LayoutJob::single_section(
        label.to_owned(),
        egui::TextFormat {
            font_id: font_id.clone(),
            color: if selected {
                ui.visuals().strong_text_color()
            } else {
                egui::Color32::from_rgb(170, 179, 193)
            },
            ..Default::default()
        },
    );
    job.wrap.max_width = TAB_MAX_WIDTH - 2.0 * pad - close_size;
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    let width = (galley.size().x + 2.0 * pad + close_size).clamp(120.0, TAB_MAX_WIDTH);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::click_and_drag());
    let fill = if selected {
        theme::PANEL
    } else if response.hovered() {
        theme::HOVER
    } else {
        theme::STRIP
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - 2.0),
                rect.right_bottom(),
            ),
            0.0,
            theme::ACCENT,
        );
    }
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(1.0, theme::BORDER),
    );
    let text_pos = egui::pos2(rect.left() + pad, rect.center().y - galley.size().y * 0.5);
    ui.painter()
        .galley(text_pos, galley, ui.visuals().text_color());
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - pad - close_size * 0.5, rect.center().y),
        egui::vec2(close_size, close_size),
    );
    let close = ui.interact(close_rect, response.id.with("close"), egui::Sense::click());
    if close.hovered() {
        ui.painter().rect_filled(close_rect, 4.0, theme::HOVER);
    }
    if selected || response.hovered() || close.hovered() {
        ui.painter().text(
            close_rect.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            font_id,
            ui.visuals().text_color(),
        );
    }
    let middle_click = response.clicked_by(egui::PointerButton::Middle);
    let close = close.on_hover_text(tr(ui.ctx(), "Close tab"));
    (
        response.on_hover_text(label),
        close.clicked() || middle_click,
    )
}

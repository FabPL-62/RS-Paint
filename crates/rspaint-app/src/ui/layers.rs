//! The layer panel shown at the bottom of the right sidebar.

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use rspaint_core::{LayerBlend, PixelBuffer};

use crate::app::PaintApp;

const ROW_HEIGHT: f32 = 34.0;
const INDENT: f32 = 12.0;
const EYE_SIZE: f32 = 18.0;
const THUMB_WIDTH: f32 = 34.0;
const THUMB_HEIGHT: f32 = 24.0;
const LOCK_SIZE: f32 = 18.0;
const ARROW_SIZE: f32 = 14.0;
const ACTION_SIZE: f32 = 24.0;

/// Transient state of the layer panel (not part of the document).
#[derive(Default)]
pub struct LayersUiState {
    renaming: Option<u64>,
    rename_text: String,
    rename_focus: bool,
    dragging: Option<u64>,
    thumbnails: std::collections::HashMap<u64, (u64, egui::TextureHandle)>,
}

impl LayersUiState {
    /// Whether a layer name is being edited, so global shortcuts must not
    /// consume the keyboard.
    pub fn is_renaming(&self) -> bool {
        self.renaming.is_some()
    }
}

/// One drawn row, used for drag-and-drop hit testing.
struct RowInfo {
    id: u64,
    rect: Rect,
    depth: usize,
    is_group: bool,
    expanded: bool,
}

struct RowMeta {
    id: u64,
    depth: usize,
    is_group: bool,
    expanded: bool,
    name: String,
    visible: bool,
    locked: bool,
    revision: u64,
}

/// What a drop would do.
enum DropTarget {
    RootStart,
    RootEnd,
    Before(u64),
    After(u64),
    Into(u64),
}

/// Draws the layer section: properties, actions and the draggable layer list.
pub fn show(app: &mut PaintApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    // A document change may have removed the row being renamed or dragged.
    if let Some(id) = app.layers_ui.renaming {
        if app.editor.layer_node(id).is_none() {
            app.layers_ui.renaming = None;
            app.layers_ui.rename_text.clear();
        }
    }
    if let Some(id) = app.layers_ui.dragging {
        if app.editor.layer_node(id).is_none() {
            app.layers_ui.dragging = None;
        }
    }
    ui.add_space(2.0);
    ui.label(egui::RichText::new(crate::i18n::tr(&ctx, "Layers")).strong());
    active_controls(app, ui);
    action_row(app, ui);
    ui.add_space(2.0);

    let active = app.editor.active_layer_id();
    let selection: Vec<u64> = app.editor.layer_selection().to_vec();
    let rows_meta: Vec<RowMeta> = app
        .editor
        .layer_rows()
        .iter()
        .map(|row| RowMeta {
            id: row.node.id(),
            depth: row.depth,
            is_group: row.node.is_group(),
            expanded: row.node.expanded(),
            name: row.node.name().to_owned(),
            visible: row.node.visible(),
            locked: row.node.locked(),
            revision: app.editor.layer_revision(row.node.id()),
        })
        .collect();

    let mut rows: Vec<RowInfo> = Vec::new();
    let mut released = false;
    egui::ScrollArea::vertical()
        .id_salt("layers-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for meta in &rows_meta {
                let selected = selection.contains(&meta.id);
                let is_active = meta.id == active;
                draw_row(app, ui, meta, selected, is_active, &mut rows, &mut released);
            }
            if app.layers_ui.dragging.is_some() {
                paint_drop_indicator(ui, &rows);
            }
        });

    if released {
        finish_drag(app, &ctx, &rows);
    } else if app.layers_ui.dragging.is_some() {
        paint_drag_ghost(app, &ctx, &rows_meta);
    }

    // Forget thumbnails of deleted layers.
    let ids: Vec<u64> = rows_meta.iter().map(|meta| meta.id).collect();
    app.layers_ui.thumbnails.retain(|id, _| ids.contains(id));
}

fn active_controls(app: &mut PaintApp, ui: &mut egui::Ui) {
    let Some((active, mut blend, opacity)) = app
        .editor
        .active_node()
        .map(|node| (app.editor.active_layer_id(), node.blend(), node.opacity()))
    else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr(ui.ctx(), "Blend mode"));
        egui::ComboBox::from_id_salt("layer-blend")
            .width(120.0)
            .selected_text(crate::i18n::tr(ui.ctx(), blend.label()))
            .show_ui(ui, |ui| {
                for option in LayerBlend::ALL {
                    ui.selectable_value(
                        &mut blend,
                        option,
                        crate::i18n::tr(ui.ctx(), option.label()),
                    );
                }
            });
    });
    if Some(blend) != app.editor.active_node().map(|node| node.blend()) {
        app.editor.set_layer_blend(active, blend);
    }
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr(ui.ctx(), "Opacity"));
        let mut value = u32::from(opacity);
        let response = ui.add(
            egui::Slider::new(&mut value, 0..=100)
                .suffix("%")
                .show_value(true),
        );
        if value != u32::from(opacity) {
            app.editor.set_layer_opacity(active, value as u8);
        }
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            app.editor.commit("Layer Opacity");
        }
    });
}

/// Responses of the action buttons; tests use the rectangles to click them.
#[cfg_attr(not(test), allow(dead_code))]
struct ActionResponses {
    add: egui::Response,
    delete: egui::Response,
}

fn action_row(app: &mut PaintApp, ui: &mut egui::Ui) -> ActionResponses {
    let mut responses = None;
    ui.horizontal(|ui| {
        let add = icon_button(
            ui,
            ActionIcon::Plus,
            crate::i18n::tr(ui.ctx(), "Add layer"),
            true,
        );
        if add.clicked() {
            app.editor.add_layer();
        }
        let group = icon_button(
            ui,
            ActionIcon::Folder,
            crate::i18n::tr(ui.ctx(), "Add group"),
            true,
        );
        if group.clicked() {
            app.editor.add_group();
        }
        let can_delete = app.editor.can_delete_layers();
        let delete = icon_button(
            ui,
            ActionIcon::Trash,
            crate::i18n::tr(ui.ctx(), "Delete layer"),
            can_delete,
        );
        if delete.clicked() {
            app.editor.delete_selected_layers();
        }
        if app.editor.layer_selection().len() > 1
            && icon_button(
                ui,
                ActionIcon::Merge,
                crate::i18n::tr(ui.ctx(), "Merge layers"),
                true,
            )
            .clicked()
        {
            app.editor.merge_selected_layers();
        }
        responses = Some(ActionResponses { add, delete });
    });
    responses.expect("the action row is always drawn")
}

fn draw_row(
    app: &mut PaintApp,
    ui: &mut egui::Ui,
    meta: &RowMeta,
    selected: bool,
    is_active: bool,
    rows: &mut Vec<RowInfo>,
    released: &mut bool,
) {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click_and_drag());
    rows.push(RowInfo {
        id: meta.id,
        rect,
        depth: meta.depth,
        is_group: meta.is_group,
        expanded: meta.expanded,
    });

    let left = rect.left() + 6.0;
    let eye_rect = Rect::from_center_size(
        Pos2::new(left + EYE_SIZE / 2.0, rect.center().y),
        Vec2::splat(EYE_SIZE),
    );
    let lock_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 6.0 - LOCK_SIZE / 2.0, rect.center().y),
        Vec2::splat(LOCK_SIZE),
    );
    let content_left = left + EYE_SIZE + 4.0 + INDENT * meta.depth as f32;
    let arrow_rect = meta.is_group.then(|| {
        Rect::from_center_size(
            Pos2::new(content_left + ARROW_SIZE / 2.0, rect.center().y),
            Vec2::splat(ARROW_SIZE),
        )
    });
    let thumb_left = content_left + if meta.is_group { ARROW_SIZE + 2.0 } else { 0.0 };
    let thumb_rect = Rect::from_min_size(
        Pos2::new(thumb_left, rect.center().y - THUMB_HEIGHT / 2.0),
        Vec2::new(THUMB_WIDTH, THUMB_HEIGHT),
    );
    let name_rect = Rect::from_min_max(
        Pos2::new(thumb_rect.right() + 6.0, rect.top() + 2.0),
        Pos2::new(lock_rect.left() - 4.0, rect.bottom() - 2.0),
    );

    let row_id = ui.make_persistent_id(("layer-row", meta.id));
    let eye = ui.interact(eye_rect, row_id.with("eye"), Sense::click());
    let lock = ui.interact(lock_rect, row_id.with("lock"), Sense::click());
    let arrow = arrow_rect.map(|rect| ui.interact(rect, row_id.with("arrow"), Sense::click()));
    let over_control = eye.hovered()
        || lock.hovered()
        || arrow.as_ref().is_some_and(|response| response.hovered());

    // Background.
    let painter = ui.painter();
    if is_active {
        painter.rect_filled(rect, 3.0, crate::theme::ICON_SELECTED);
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
            0.0,
            crate::theme::ACCENT,
        );
    } else if selected {
        painter.rect_filled(rect, 3.0, crate::theme::HOVER);
    } else if response.hovered() {
        painter.rect_filled(
            rect,
            3.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, 10),
        );
    }

    // Thumbnail.
    paint_checker(painter, thumb_rect);
    if meta.is_group {
        paint_folder(painter, thumb_rect.shrink(4.0), Color32::from_gray(175));
    } else {
        ensure_thumbnail(app, ui.ctx(), meta);
        if let Some((_, texture)) = app.layers_ui.thumbnails.get(&meta.id) {
            painter.image(
                texture.id(),
                thumb_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
    painter.rect_stroke(
        thumb_rect,
        1.0,
        Stroke::new(1.0_f32, crate::theme::BORDER),
        StrokeKind::Inside,
    );

    // Name.
    let text_color = if meta.visible {
        Color32::from_gray(220)
    } else {
        Color32::from_gray(135)
    };
    if app.layers_ui.renaming != Some(meta.id) {
        ui.painter().with_clip_rect(name_rect).text(
            Pos2::new(name_rect.left(), name_rect.center().y),
            Align2::LEFT_CENTER,
            &meta.name,
            FontId::proportional(13.0),
            text_color,
        );
    }

    // Eye / expand arrow / lock.
    paint_eye(ui.painter(), eye_rect, meta.visible, text_color);
    if let Some(arrow_rect) = arrow_rect {
        paint_arrow(ui.painter(), arrow_rect, meta.expanded);
    }
    paint_lock(ui.painter(), lock_rect, meta.locked, text_color);

    if eye.clicked() {
        app.editor.toggle_layer_visible(meta.id);
        return;
    }
    if lock.clicked() {
        app.editor.toggle_layer_locked(meta.id);
        return;
    }
    if arrow.as_ref().is_some_and(|response| response.clicked()) {
        app.editor.toggle_group_expanded(meta.id);
        return;
    }

    // Rename editor.
    if app.layers_ui.renaming == Some(meta.id) {
        let mut text = std::mem::take(&mut app.layers_ui.rename_text);
        let edit = egui::TextEdit::singleline(&mut text)
            .id(row_id.with("rename"))
            .font(FontId::proportional(13.0))
            .desired_width(name_rect.width());
        let edit_response = ui.put(name_rect, edit);
        app.layers_ui.rename_text = text;
        if app.layers_ui.rename_focus {
            edit_response.request_focus();
            app.layers_ui.rename_focus = false;
        }
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if escape {
            app.layers_ui.renaming = None;
            app.layers_ui.rename_text.clear();
        } else if enter || edit_response.lost_focus() {
            let name = app.layers_ui.rename_text.clone();
            app.layers_ui.renaming = None;
            app.layers_ui.rename_text.clear();
            app.editor.rename_layer(meta.id, &name);
        }
        return;
    }

    if response.double_clicked()
        && response
            .interact_pointer_pos()
            .is_some_and(|pos| name_rect.contains(pos))
        && !over_control
    {
        app.layers_ui.renaming = Some(meta.id);
        app.layers_ui.rename_text = meta.name.clone();
        app.layers_ui.rename_focus = true;
        return;
    }

    if response.drag_started() && !over_control {
        app.layers_ui.dragging = Some(meta.id);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if response.drag_stopped() {
        *released = true;
    }

    if response.clicked() && !over_control {
        let (shift, ctrl) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        app.editor.select_layer(meta.id, shift, ctrl);
    }
}

fn ensure_thumbnail(app: &mut PaintApp, ctx: &egui::Context, meta: &RowMeta) {
    if meta.is_group {
        return;
    }
    let needs_update = app
        .layers_ui
        .thumbnails
        .get(&meta.id)
        .map(|(revision, _)| *revision != meta.revision)
        .unwrap_or(true);
    if !needs_update {
        return;
    }
    let Some((size, bytes)) = app
        .editor
        .layer_pixels(meta.id)
        .map(|pixels| thumbnail_rgba(pixels, THUMB_WIDTH, THUMB_HEIGHT))
    else {
        return;
    };
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &bytes);
    match app.layers_ui.thumbnails.get_mut(&meta.id) {
        Some((revision, texture)) => {
            texture.set(image, egui::TextureOptions::NEAREST);
            *revision = meta.revision;
        }
        None => {
            let texture = ctx.load_texture(
                format!("layer-thumb-{}", meta.id),
                image,
                egui::TextureOptions::NEAREST,
            );
            app.layers_ui
                .thumbnails
                .insert(meta.id, (meta.revision, texture));
        }
    }
}

/// Scales a layer to fit the thumbnail box with nearest-neighbor sampling.
fn thumbnail_rgba(pixels: &PixelBuffer, max_width: f32, max_height: f32) -> ([usize; 2], Vec<u8>) {
    let width = pixels.width().max(1);
    let height = pixels.height().max(1);
    let scale = (max_width / width as f32).min(max_height / height as f32);
    let tw = ((width as f32 * scale).round().max(1.0) as u32).max(1);
    let th = ((height as f32 * scale).round().max(1.0) as u32).max(1);
    let mut bytes = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        let sy = (((y as f32 + 0.5) / scale) as u32).min(height - 1);
        for x in 0..tw {
            let sx = (((x as f32 + 0.5) / scale) as u32).min(width - 1);
            let color = pixels
                .get(sx, sy)
                .unwrap_or(rspaint_core::Color::TRANSPARENT);
            bytes.extend_from_slice(&[color.r, color.g, color.b, color.a]);
        }
    }
    ([tw as usize, th as usize], bytes)
}

fn paint_checker(painter: &egui::Painter, rect: Rect) {
    let cell = 6.0;
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 1.0, Color32::from_gray(72));
    let mut y = rect.top();
    let mut row = 0;
    while y < rect.bottom() {
        let mut x = rect.left();
        let mut column = 0;
        while x < rect.right() {
            if (row + column) % 2 == 0 {
                let max = Pos2::new((x + cell).min(rect.right()), (y + cell).min(rect.bottom()));
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x, y), max),
                    0.0,
                    Color32::from_gray(96),
                );
            }
            x += cell;
            column += 1;
        }
        y += cell;
        row += 1;
    }
}

fn paint_folder(painter: &egui::Painter, rect: Rect, color: Color32) {
    let rect = Rect::from_center_size(rect.center(), Vec2::new(18.0, 14.0));
    let tab = Rect::from_min_size(rect.min, Vec2::new(8.0, 3.0));
    painter.rect_filled(tab, 1.0, color);
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(rect.left(), rect.top() + 2.0), rect.max),
        1.5,
        color,
    );
    painter.rect_stroke(
        Rect::from_min_max(Pos2::new(rect.left(), rect.top() + 2.0), rect.max),
        1.5,
        Stroke::new(1.0_f32, color.gamma_multiply(0.7)),
        StrokeKind::Inside,
    );
}

fn paint_eye(painter: &egui::Painter, rect: Rect, visible: bool, color: Color32) {
    let center = rect.center();
    let stroke = Stroke::new(1.3_f32, color);
    if visible {
        painter.add(egui::Shape::ellipse_stroke(
            center,
            Vec2::new(7.0, 4.6),
            stroke,
        ));
        painter.circle_filled(center, 2.1, color);
    } else {
        let dim = color.gamma_multiply(0.65);
        painter.add(egui::Shape::ellipse_stroke(
            center,
            Vec2::new(7.0, 4.6),
            Stroke::new(1.0_f32, dim),
        ));
        painter.line_segment(
            [center + Vec2::new(-7.5, 6.0), center + Vec2::new(7.5, -6.0)],
            Stroke::new(1.4_f32, color),
        );
    }
}

fn paint_lock(painter: &egui::Painter, rect: Rect, locked: bool, color: Color32) {
    let center = rect.center();
    let body = Rect::from_center_size(center + Vec2::new(0.0, 2.5), Vec2::new(10.0, 7.0));
    painter.rect_stroke(body, 1.5, Stroke::new(1.2_f32, color), StrokeKind::Inside);
    let stroke = Stroke::new(1.2_f32, color);
    if locked {
        painter.line_segment(
            [
                Pos2::new(center.x - 3.0, center.y + 1.0),
                Pos2::new(center.x - 3.0, center.y - 3.5),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(center.x - 3.0, center.y - 3.5),
                Pos2::new(center.x + 3.0, center.y - 3.5),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(center.x + 3.0, center.y - 3.5),
                Pos2::new(center.x + 3.0, center.y + 1.0),
            ],
            stroke,
        );
    } else {
        painter.line_segment(
            [
                Pos2::new(center.x - 3.0, center.y + 1.0),
                Pos2::new(center.x - 3.0, center.y - 3.5),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(center.x - 3.0, center.y - 3.5),
                Pos2::new(center.x + 4.0, center.y - 3.5),
            ],
            stroke,
        );
    }
}

fn paint_arrow(painter: &egui::Painter, rect: Rect, expanded: bool) {
    let center = rect.center();
    let points = if expanded {
        vec![
            center + Vec2::new(-4.0, -2.0),
            center + Vec2::new(4.0, -2.0),
            center + Vec2::new(0.0, 3.0),
        ]
    } else {
        vec![
            center + Vec2::new(-2.0, -4.0),
            center + Vec2::new(3.0, 0.0),
            center + Vec2::new(-2.0, 4.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        Color32::from_gray(190),
        Stroke::NONE,
    ));
}

#[derive(Clone, Copy)]
enum ActionIcon {
    Plus,
    Folder,
    Trash,
    Merge,
}

fn icon_button(
    ui: &mut egui::Ui,
    icon: ActionIcon,
    tooltip: &str,
    enabled: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ACTION_SIZE), Sense::click());
    let color = if enabled {
        Color32::from_gray(205)
    } else {
        Color32::from_gray(90)
    };
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            4.0,
            if enabled && response.hovered() {
                crate::theme::HOVER
            } else {
                Color32::TRANSPARENT
            },
        );
        let stroke = Stroke::new(1.3_f32, color);
        let center = rect.center();
        match icon {
            ActionIcon::Plus => {
                painter.line_segment(
                    [center + Vec2::new(-5.0, 0.0), center + Vec2::new(5.0, 0.0)],
                    stroke,
                );
                painter.line_segment(
                    [center + Vec2::new(0.0, -5.0), center + Vec2::new(0.0, 5.0)],
                    stroke,
                );
            }
            ActionIcon::Folder => paint_folder(painter, rect.shrink(5.0), color),
            ActionIcon::Trash => {
                painter.line_segment(
                    [
                        center + Vec2::new(-5.5, -5.0),
                        center + Vec2::new(5.5, -5.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        center + Vec2::new(-2.0, -5.0),
                        center + Vec2::new(-2.0, -7.0),
                    ],
                    stroke,
                );
                painter.rect_stroke(
                    Rect::from_center_size(center + Vec2::new(0.0, 1.0), Vec2::new(8.0, 9.0)),
                    1.0,
                    stroke,
                    StrokeKind::Inside,
                );
                painter.line_segment(
                    [
                        center + Vec2::new(-2.0, -2.0),
                        center + Vec2::new(-2.0, 4.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [center + Vec2::new(2.0, -2.0), center + Vec2::new(2.0, 4.0)],
                    stroke,
                );
            }
            ActionIcon::Merge => {
                painter.rect_stroke(
                    Rect::from_center_size(center + Vec2::new(-2.5, -2.5), Vec2::splat(8.0)),
                    1.0,
                    stroke,
                    StrokeKind::Inside,
                );
                painter.rect_stroke(
                    Rect::from_center_size(center + Vec2::new(2.5, 2.5), Vec2::splat(8.0)),
                    1.0,
                    stroke,
                    StrokeKind::Inside,
                );
            }
        }
    }
    if enabled {
        response.on_hover_text(tooltip)
    } else {
        response
    }
}

fn pointer_pos(ctx: &egui::Context) -> Option<Pos2> {
    ctx.input(|i| i.pointer.interact_pos())
}

fn drop_target(rows: &[RowInfo], pos: Pos2) -> Option<DropTarget> {
    let top = rows.first()?.rect.top();
    let bottom = rows.last()?.rect.bottom();
    if pos.y < top || pos.y > bottom {
        return None;
    }
    if let Some(row) = rows.iter().find(|row| row.rect.contains(pos)) {
        let relative = (pos.y - row.rect.top()) / row.rect.height();
        if relative < 0.28 {
            return Some(DropTarget::Before(row.id));
        }
        if relative > 0.72 {
            return Some(DropTarget::After(row.id));
        }
        if row.is_group && row.expanded {
            return Some(DropTarget::Into(row.id));
        }
        return Some(DropTarget::Before(row.id));
    }
    if pos.y <= top {
        Some(DropTarget::RootStart)
    } else {
        Some(DropTarget::RootEnd)
    }
}

fn finish_drag(app: &mut PaintApp, ctx: &egui::Context, rows: &[RowInfo]) {
    let Some(id) = app.layers_ui.dragging.take() else {
        return;
    };
    let Some(pos) = pointer_pos(ctx) else {
        return;
    };
    let Some(target) = drop_target(rows, pos) else {
        return;
    };
    match target {
        DropTarget::RootStart => app.editor.move_layer_to_start(id),
        DropTarget::RootEnd => app.editor.move_layer_to_end(id),
        DropTarget::Before(target) if target != id => app.editor.move_layer_before(id, target),
        DropTarget::After(target) if target != id => app.editor.move_layer_after(id, target),
        DropTarget::Into(group) if group != id => app.editor.move_layer_into(id, group),
        _ => {}
    }
}

fn paint_drop_indicator(ui: &egui::Ui, rows: &[RowInfo]) {
    let Some(pos) = pointer_pos(ui.ctx()) else {
        return;
    };
    let Some(target) = drop_target(rows, pos) else {
        return;
    };
    let painter = ui.painter();
    let line = |y: f32, x0: f32| {
        painter.line_segment(
            [Pos2::new(x0, y), Pos2::new(ui.clip_rect().right() - 8.0, y)],
            Stroke::new(2.0_f32, crate::theme::ACCENT),
        );
    };
    match target {
        DropTarget::RootStart => {
            if let Some(first) = rows.first() {
                line(first.rect.top(), first.rect.left());
            }
        }
        DropTarget::RootEnd => {
            if let Some(last) = rows.last() {
                line(last.rect.bottom(), last.rect.left());
            }
        }
        DropTarget::Before(id) => {
            if let Some(row) = rows.iter().find(|row| row.id == id) {
                line(
                    row.rect.top(),
                    row.rect.left() + 6.0 + INDENT * row.depth as f32,
                );
            }
        }
        DropTarget::After(id) => {
            if let Some(row) = rows.iter().find(|row| row.id == id) {
                line(
                    row.rect.bottom(),
                    row.rect.left() + 6.0 + INDENT * row.depth as f32,
                );
            }
        }
        DropTarget::Into(id) => {
            if let Some(row) = rows.iter().find(|row| row.id == id) {
                painter.rect_stroke(
                    row.rect,
                    3.0,
                    Stroke::new(2.0_f32, crate::theme::ACCENT),
                    StrokeKind::Inside,
                );
            }
        }
    }
}

fn paint_drag_ghost(app: &PaintApp, ctx: &egui::Context, rows: &[RowMeta]) {
    let Some(id) = app.layers_ui.dragging else {
        return;
    };
    let Some(name) = rows
        .iter()
        .find(|row| row.id == id)
        .map(|row| row.name.clone())
    else {
        return;
    };
    let Some(pos) = pointer_pos(ctx) else {
        return;
    };
    egui::Area::new(egui::Id::new("layer-drag-ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + Vec2::new(12.0, 8.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::theme::PANEL)
                .stroke(Stroke::new(1.0_f32, crate::theme::ACCENT))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.label(name);
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1024.0, 768.0),
            )),
            events,
            ..Default::default()
        }
    }

    fn press(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]
    }

    fn release(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]
    }

    #[test]
    fn action_buttons_add_and_delete_layers() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut app = PaintApp::new_blank(false, false, &ctx);

        let mut add_rect = None;
        let _ = ctx.run(frame(Vec::new()), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                add_rect = Some(action_row(&mut app, ui).add.rect);
            });
        });
        let pos = add_rect.unwrap().center();
        let _ = ctx.run(frame(press(pos)), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                action_row(&mut app, ui);
            });
        });
        let _ = ctx.run(frame(release(pos)), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                action_row(&mut app, ui);
            });
        });
        assert_eq!(app.editor.layers().len(), 2);

        let mut delete_rect = None;
        let _ = ctx.run(frame(Vec::new()), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                delete_rect = Some(action_row(&mut app, ui).delete.rect);
            });
        });
        let pos = delete_rect.unwrap().center();
        let _ = ctx.run(frame(press(pos)), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                action_row(&mut app, ui);
            });
        });
        let _ = ctx.run(frame(release(pos)), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                action_row(&mut app, ui);
            });
        });
        assert_eq!(app.editor.layers().len(), 1);
    }
}

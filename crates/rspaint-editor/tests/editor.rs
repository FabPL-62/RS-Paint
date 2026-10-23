//! Exercise editing workflows through the public API, without UI or GPU.
use rspaint_core::{Color, PixelBuffer};
use rspaint_editor::{tools::Tool, Editor, FlipRotate, PointerButton, ResizeHandle};

fn editor() -> Editor {
    let mut editor = Editor::new();
    editor.load_buffer(PixelBuffer::new(16, 16, Color::WHITE), None);
    editor
}

#[test]
fn opened_image_uses_exact_top_left_color_as_its_secondary_color() {
    let mut e = editor();
    let top_left = Color::rgba(60, 120, 180, 75);
    let mut image = PixelBuffer::new(3, 2, Color::WHITE);
    image.set(0, 0, top_left);
    e.load_buffer(image, Some("sample.png".into()));
    assert_eq!(e.colors.background, top_left);
    assert!(!e.is_modified());
    assert!(e.document.transparent);
}

#[test]
fn sharing_tool_options_never_overwrites_another_documents_colors_or_palettes() {
    let mut first = editor();
    let mut second = editor();
    first.colors.foreground = Color::rgb(200, 0, 0);
    first.colors.background = Color::rgb(0, 150, 0);
    first.colors.palette[0] = Color::rgb(30, 50, 70);
    first.colors.captured[3] = Color::WHITE;
    first.settings.brush_size = 21;
    second.colors.foreground = Color::rgb(0, 0, 200);
    second.colors.background = Color::rgb(100, 90, 80);
    let palette = second.colors.palette.clone();
    let captured = second.colors.captured.clone();
    second.apply_shared(&first.capture_shared());
    assert_eq!(second.settings.brush_size, 21);
    assert_eq!(second.colors.foreground, Color::rgb(0, 0, 200));
    assert_eq!(second.colors.background, Color::rgb(100, 90, 80));
    assert_eq!(second.colors.palette, palette);
    assert_eq!(second.colors.captured, captured);
    assert_eq!(first.colors.palette[0], Color::rgb(30, 50, 70));
}

#[test]
fn captured_palette_retains_all_colors_beyond_two_rows_and_reset_is_independent() {
    let mut e = editor();
    assert_eq!(e.colors.captured, vec![Color::BLACK; 28]);
    let mut buffer = PixelBuffer::new(70, 1, Color::WHITE);
    for x in 0..70 {
        buffer.set(x, 0, Color::from_hsv((69 - x) as f32 * 5.0, 1.0, 1.0));
    }
    e.load_buffer(buffer, None);
    let expected = rspaint_core::palette::colors_by_hue(&e.document.buffer);
    assert_eq!(expected.len(), 70);
    e.capture_colors();
    assert_eq!(e.colors.captured, expected);
    e.colors.palette[0] = Color::WHITE;
    e.reset_colors();
    assert_eq!(e.colors.palette, rspaint_core::palette::default_palette());
    assert_eq!(e.colors.captured, expected);
    // Painting does not overwrite colors collected from the image.
    e.begin_stroke((0, 0), PointerButton::Primary);
    e.end_stroke(false);
    assert_eq!(e.colors.captured, expected);
}

fn select(editor: &mut Editor, start: (i32, i32), end: (i32, i32)) {
    editor.set_tool(Tool::Select);
    editor.begin_selection_drag(start, Tool::Select, false, false, false);
    editor.update_selection_drag(end);
    editor.end_selection_drag();
}

#[test]
fn pencil_stroke_undo_redo_preserves_pixels() {
    let mut e = editor();
    let initial = e.document.buffer.clone();
    e.set_tool(Tool::Pencil);
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.update_stroke((6, 2), false);
    e.end_stroke(false);
    let drawn = e.document.buffer.clone();
    for x in 2..=6 {
        assert_eq!(drawn.get(x, 2), Some(Color::BLACK));
    }
    assert_eq!(drawn.get(1, 2), Some(Color::WHITE));
    assert!(e.is_modified());
    e.undo();
    assert_eq!(e.document.buffer, initial);
    e.redo();
    assert_eq!(e.document.buffer, drawn);
}

#[test]
fn selection_clips_drawing_without_losing_the_selection() {
    let mut e = editor();
    select(&mut e, (3, 3), (5, 5));
    e.set_tool(Tool::Pencil);
    e.begin_stroke((0, 4), PointerButton::Primary);
    e.update_stroke((10, 4), false);
    e.end_stroke(false);
    assert!(e.selection.is_some());
    for x in 0..16 {
        let expected = if (3..=5).contains(&x) {
            Color::BLACK
        } else {
            Color::WHITE
        };
        assert_eq!(e.document.buffer.get(x, 4), Some(expected), "pixel {x}");
    }
}

#[test]
fn dragging_selection_without_ctrl_moves_only_the_outline() {
    let mut e = editor();
    e.document.buffer.set(2, 2, Color::BLACK);
    select(&mut e, (2, 2), (3, 3));

    e.begin_selection_drag((2, 2), Tool::Select, false, false, false);
    e.update_selection_drag((2, 5));
    e.end_selection_drag();

    assert_eq!(e.document.buffer.get(2, 2), Some(Color::BLACK));
    assert_eq!(e.document.buffer.get(2, 5), Some(Color::WHITE));
    assert!(!e.point_in_selection((2, 2)));
    assert!(e.point_in_selection((2, 5)));
}

#[test]
fn ctrl_drag_moves_the_whole_multiselection() {
    let mut e = editor();
    e.document.buffer.set(1, 1, Color::BLACK);
    e.document.buffer.set(8, 1, Color::BLACK);
    select(&mut e, (1, 1), (2, 2));
    e.begin_selection_drag((8, 1), Tool::Select, true, false, false);
    e.update_selection_drag((9, 2));
    e.end_selection_drag();

    e.begin_selection_drag((1, 1), Tool::Select, false, false, true);
    e.update_selection_drag((1, 5));
    e.end_selection_drag();

    assert_eq!(e.document.buffer.get(1, 1), Some(Color::WHITE));
    assert_eq!(e.document.buffer.get(8, 1), Some(Color::WHITE));
    assert_eq!(e.document.buffer.get(1, 5), Some(Color::BLACK));
    assert_eq!(e.document.buffer.get(8, 5), Some(Color::BLACK));
}

#[test]
fn ctrl_drag_keeps_a_floating_multiselection_together() {
    let mut e = editor();
    e.document.buffer.set(1, 1, Color::BLACK);
    e.document.buffer.set(8, 1, Color::BLACK);
    select(&mut e, (1, 1), (2, 2));
    e.begin_selection_drag((8, 1), Tool::Select, true, false, false);
    e.update_selection_drag((9, 2));
    e.end_selection_drag();

    e.begin_selection_drag((1, 1), Tool::Select, false, false, true);
    e.update_selection_drag((1, 3));
    e.end_selection_drag();
    assert_eq!(e.document.buffer.get(1, 3), Some(Color::BLACK));
    assert_eq!(e.document.buffer.get(8, 3), Some(Color::BLACK));

    e.begin_selection_drag((1, 3), Tool::Select, false, false, true);
    e.update_selection_drag((1, 5));
    e.end_selection_drag();

    assert_eq!(e.document.buffer.get(1, 3), Some(Color::WHITE));
    assert_eq!(e.document.buffer.get(8, 3), Some(Color::WHITE));
    assert_eq!(e.document.buffer.get(1, 5), Some(Color::BLACK));
    assert_eq!(e.document.buffer.get(8, 5), Some(Color::BLACK));
}

#[test]
fn dragging_one_area_without_ctrl_keeps_every_pixel_in_place() {
    let mut e = editor();
    e.document.buffer.set(1, 1, Color::BLACK);
    e.document.buffer.set(8, 1, Color::BLACK);
    select(&mut e, (1, 1), (2, 2));
    e.begin_selection_drag((8, 1), Tool::Select, true, false, false);
    e.update_selection_drag((9, 2));
    e.end_selection_drag();

    e.begin_selection_drag((1, 1), Tool::Select, false, false, false);
    e.update_selection_drag((1, 5));
    e.end_selection_drag();

    assert_eq!(e.document.buffer.get(1, 1), Some(Color::BLACK));
    assert_eq!(e.document.buffer.get(8, 1), Some(Color::BLACK));
    assert!(!e.point_in_selection((1, 1)));
    assert!(e.point_in_selection((1, 5)));
    assert!(e.point_in_selection((8, 1)));
}

#[test]
fn hovering_a_disconnected_selection_reports_only_that_area_size() {
    let mut e = editor();
    select(&mut e, (1, 1), (2, 2));
    e.begin_selection_drag((8, 4), Tool::Select, true, false, false);
    e.update_selection_drag((10, 6));
    e.end_selection_drag();

    assert_eq!(e.selection_portion_size((1, 1)), Some((2, 2)));
    assert_eq!(e.selection_portion_size((9, 5)), Some((3, 3)));
    assert_eq!(e.selection_portion_size((5, 5)), None);
    assert_eq!(e.selection_bounds(), Some((1, 1, 10, 6)));
}

#[test]
fn free_form_selection_traces_the_selected_pixels() {
    let mut e = editor();
    e.set_tool(Tool::FreeFormSelect);
    e.begin_selection_drag((2, 2), Tool::FreeFormSelect, false, false, false);
    e.update_selection_drag((10, 2));
    e.update_selection_drag((10, 10));
    e.update_selection_drag((2, 10));
    e.update_selection_drag((2, 2));
    e.end_selection_drag();

    assert!(e.selection_outline().is_none());
    assert!(e.selection_boundary().is_some());
    assert_eq!(e.selection_bounds().unwrap().2, 9);
    assert_eq!(e.selection_bounds().unwrap().3, 9);
}

#[test]
fn secondary_pointer_uses_secondary_color() {
    let mut e = editor();
    e.colors.background = Color::rgb(240, 80, 40);
    e.begin_stroke((2, 2), PointerButton::Secondary);
    e.end_stroke(false);
    assert_eq!(e.document.buffer.get(2, 2), Some(e.colors.background));
}

#[test]
fn rectangle_preview_can_be_cancelled_without_committing() {
    let mut e = editor();
    let initial = e.document.buffer.clone();
    let node = e.history.current();
    e.set_tool(Tool::Rectangle);
    e.begin_stroke((1, 1), PointerButton::Primary);
    e.update_stroke((8, 6), false);
    assert_ne!(e.document.buffer, initial);
    e.cancel_stroke();
    assert_eq!(e.document.buffer, initial);
    assert_eq!(e.history.current(), node);
    assert!(!e.has_stroke());
}

#[test]
fn copy_cut_and_internal_paste_work_without_system_clipboard() {
    let mut e = editor();
    e.document.buffer.set(3, 3, Color::BLACK);
    select(&mut e, (3, 3), (4, 4));
    let copied = e.cut_selection().unwrap();
    assert_eq!(copied.size(), [2, 2]);
    assert_eq!(copied.get(0, 0), Some(Color::BLACK));
    assert_eq!(e.document.buffer.get(3, 3), Some(Color::WHITE));
    e.set_pointer(Some((8, 8)));
    e.paste_clipboard(None);
    assert_eq!(e.selection_pixels().unwrap(), &copied);
    e.deselect();
    assert_eq!(e.document.buffer.get(7, 7), Some(Color::BLACK));
}

#[test]
fn rotation_transforms_selection_pixels_and_is_undoable() {
    let mut e = editor();
    e.document.buffer.set(2, 2, Color::BLACK);
    select(&mut e, (2, 2), (4, 3));
    let original = e.selection_pixels().unwrap().clone();
    e.transform_selection_direct(FlipRotate::Rotate90);
    assert_eq!(e.selection_pixels().unwrap().size(), [2, 3]);
    assert_eq!(e.selection_pixels().unwrap().get(1, 0), Some(Color::BLACK));
    e.colors.palette[0] = Color::rgb(80, 50, 20);
    e.undo();
    assert_eq!(e.selection_pixels().unwrap(), &original);
    assert_eq!(e.colors.palette[0], Color::rgb(80, 50, 20));
}

#[test]
fn resize_canvas_keeps_content_anchored_and_can_be_undone() {
    let mut e = editor();
    e.document.buffer.set(0, 0, Color::BLACK);
    e.load_buffer(e.document.buffer.clone(), None);
    let original = e.document.buffer.clone();
    e.begin_canvas_resize(ResizeHandle::West, emath::pos2(0.0, 0.0), 1.0);
    e.update_canvas_resize(emath::pos2(-4.0, 0.0), emath::pos2(0.0, 0.0));
    e.finish_canvas_resize();
    assert_eq!(e.document.width(), 20);
    assert_eq!(e.document.buffer.get(4, 0), Some(Color::BLACK));
    e.undo();
    assert_eq!(e.document.buffer, original);
}

#[test]
fn changing_document_discards_old_curve_text_and_drag_snapshots() {
    let mut e = editor();
    e.set_tool(Tool::Curve);
    e.curve_pointer_down((1, 1));
    e.curve_pointer_move((8, 8));
    e.curve_pointer_up();
    assert!(!e.curve_points().is_empty());
    e.default_text_box((0, 0));
    e.begin_canvas_resize(ResizeHandle::East, emath::pos2(0.0, 0.0), 1.0);
    e.load_buffer(PixelBuffer::new(7, 5, Color::WHITE), None);
    assert!(e.curve_points().is_empty());
    assert!(!e.has_text_box());
    assert!(!e.resizing_canvas());
    e.update_canvas_resize(emath::pos2(50.0, 50.0), emath::pos2(0.0, 0.0));
    assert_eq!(e.document.buffer.size(), [7, 5]);
}

#[test]
fn new_document_clears_saved_path_and_keeps_tool_options() {
    let mut e = editor();
    e.settings.brush_size = 17;
    e.mark_saved("previous.png".into());
    e.reset_document();
    assert!(e.file_path.is_none());
    assert!(!e.is_modified());
    assert_eq!(e.settings.brush_size, 17);
}

#[test]
fn renderer_receives_only_pending_pixel_regions() {
    let mut e = editor();
    assert_eq!(
        e.take_pending_upload().unwrap().data.as_slice(),
        e.document.buffer.data()
    );
    assert!(e.take_pending_upload().is_none());
    let revision = e.revision();
    e.begin_stroke((8, 8), PointerButton::Primary);
    e.end_stroke(false);
    assert!(e.revision() > revision);
    let upload = e.take_pending_upload().unwrap();
    assert!(upload.width < 16 && upload.height < 16);
    assert_eq!(
        upload.data.as_slice(),
        e.document
            .buffer
            .region_rgba(upload.x, upload.y, upload.width, upload.height)
    );
    assert!(e.take_pending_upload().is_none());
}

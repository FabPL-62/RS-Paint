//! Exercise editing workflows through the public API, without UI or GPU.
use rspaint_core::{Color, PixelBuffer};
use rspaint_editor::{tools::Tool, Editor, FlipRotate, PointerButton, ResizeHandle};

fn editor() -> Editor {
    let mut editor = Editor::new();
    editor.load_buffer(PixelBuffer::new(16, 16, Color::WHITE), None);
    editor
}

fn select(editor: &mut Editor, start: (i32, i32), end: (i32, i32)) {
    editor.set_tool(Tool::Select);
    editor.begin_selection_drag(start, Tool::Select, false, false);
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
fn secondary_pointer_uses_secondary_color() {
    let mut e = editor();
    e.settings.background = Color::rgb(240, 80, 40);
    e.begin_stroke((2, 2), PointerButton::Secondary);
    e.end_stroke(false);
    assert_eq!(e.document.buffer.get(2, 2), Some(e.settings.background));
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
    e.undo();
    assert_eq!(e.selection_pixels().unwrap(), &original);
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
        e.take_pending_upload().unwrap().data,
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
        upload.data,
        e.document
            .buffer
            .region_rgba(upload.x, upload.y, upload.width, upload.height)
    );
    assert!(e.take_pending_upload().is_none());
}

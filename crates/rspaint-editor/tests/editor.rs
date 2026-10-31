//! Exercise editing workflows through the public API, without UI or GPU.
use rspaint_core::{Color, PixelBuffer};
use rspaint_editor::{tools::Tool, Editor, FlipRotate, PointerButton, ResizeHandle};

fn editor() -> Editor {
    let mut editor = Editor::new();
    editor.load_buffer(PixelBuffer::new(16, 16, Color::WHITE), None);
    editor
}

fn check_full_composite_reference(e: &Editor) {
    let (w, h) = (e.document.width(), e.document.height());
    let mut expected = PixelBuffer::transparent(w, h);
    rspaint_core::layer::composite_region(
        &e.document.layers,
        e.document.active,
        &e.document.buffer,
        rspaint_core::layer::Region::new(0, 0, w, h),
        &mut expected,
    );
    assert_eq!(e.document.composite, expected);
}

#[test]
fn separated_upload_batches_preserve_gaps_for_all_modes_and_groups() {
    for mode in rspaint_core::LayerBlend::ALL {
        for grouped in [false, true] {
            let mut e = editor();
            e.load_buffer(PixelBuffer::new(512, 384, Color::WHITE), None);
            let group = if grouped {
                e.add_group();
                Some(e.active_layer_id())
            } else {
                None
            };
            e.add_layer();
            let active = e.active_layer_id();
            e.document.buffer.set(0, 0, Color::rgba(20, 80, 140, 180));
            e.document.buffer.set(511, 383, Color::rgba(140, 80, 20, 1));
            e.commit("Corners");
            let target = group.unwrap_or(active);
            e.set_layer_blend(target, mode);
            e.invalidate_canvas();
            drop(e.take_pending_uploads());
            for operation in 0..3 {
                match operation {
                    0 => e.set_layer_opacity(target, 37),
                    _ => e.toggle_layer_visible(target),
                }
                let patches = e.take_pending_uploads();
                assert_eq!(patches.len(), 2, "{mode:?}, grouped {grouped}");
                assert_eq!(patches.iter().map(|p| p.data.len()).sum::<usize>(), 8);
                check_full_composite_reference(&e);
                for patch in patches {
                    assert_eq!(
                        patch.data.as_slice(),
                        e.document.composite.region_rgba(
                            patch.x,
                            patch.y,
                            patch.width,
                            patch.height
                        )
                    );
                }
                assert!(e.take_pending_uploads().is_empty());
            }
        }
    }
}

#[test]
fn separated_pending_updates_merge_traces_and_full_history_restores() {
    let mut e = editor();
    e.load_buffer(PixelBuffer::new(512, 384, Color::WHITE), None);
    e.add_layer();
    let layer = e.active_layer_id();
    e.document.buffer.set(0, 0, Color::BLACK);
    e.document.buffer.set(511, 383, Color::BLACK);
    e.commit("Corners");
    e.invalidate_canvas();
    drop(e.take_pending_uploads());
    // Repeated slider events should coalesce the same patches, not hit the cap.
    for value in 50..90 {
        e.set_layer_opacity(layer, value);
    }
    e.begin_stroke((200, 200), PointerButton::Primary);
    e.end_stroke(false);
    let patches = e.take_pending_uploads();
    assert_eq!(patches.len(), 3);
    check_full_composite_reference(&e);
    e.set_layer_opacity(layer, 37);
    e.undo();
    let patches = e.take_pending_uploads();
    assert_eq!(patches.len(), 1);
    assert_eq!((patches[0].width, patches[0].height), (512, 384));
    assert!(std::sync::Arc::ptr_eq(
        &patches[0].data,
        &e.document.composite.shared_data()
    ));
    check_full_composite_reference(&e);
    // Existing single-upload clients still receive all corners in one image.
    e.set_layer_opacity(layer, 73);
    let upload = e.take_pending_upload().unwrap();
    assert_eq!(
        (upload.x, upload.y, upload.width, upload.height),
        (0, 0, 512, 384)
    );
    assert_eq!(upload.data.as_slice(), e.document.composite.data());
    check_full_composite_reference(&e);
}

#[test]
fn dense_layer_metadata_preserves_shared_full_canvas_uploads() {
    let mut e = editor();
    e.add_layer();
    let layer = e.active_layer_id();
    e.document.buffer.fill(Color::rgba(30, 80, 140, 180));
    e.commit("Dense");
    e.invalidate_canvas();
    drop(e.take_pending_upload());
    for op in 0..4 {
        match op {
            0 => e.set_layer_opacity(layer, 37),
            1 | 2 => e.toggle_layer_visible(layer),
            _ => e.set_layer_blend(layer, rspaint_core::LayerBlend::Multiply),
        }
        let upload = e.take_pending_upload().unwrap();
        assert_eq!(
            (upload.x, upload.y, upload.width, upload.height),
            (0, 0, 16, 16)
        );
        assert!(std::sync::Arc::ptr_eq(
            &upload.data,
            &e.document.composite.shared_data()
        ));
        check_full_composite_reference(&e);
    }
}

#[test]
fn sparse_metadata_uploads_only_alpha_bounds_for_all_modes_and_groups() {
    for mode in rspaint_core::LayerBlend::ALL {
        for grouped in [false, true] {
            let mut e = editor();
            e.load_buffer(PixelBuffer::new(40, 30, Color::WHITE), None);
            let group = if grouped {
                e.add_group();
                Some(e.active_layer_id())
            } else {
                None
            };
            e.add_layer();
            let active = e.active_layer_id();
            e.document.buffer.set(7, 8, Color::rgba(20, 80, 140, 1));
            e.document.buffer.set(11, 12, Color::rgba(200, 40, 70, 180));
            e.document.buffer.set(29, 25, Color::rgba(30, 60, 90, 0));
            e.commit("Sparse fixture");
            let target = group.unwrap_or(active);
            e.set_layer_blend(target, mode);
            e.invalidate_canvas();
            drop(e.take_pending_upload());
            for operation in 0..4 {
                match operation {
                    0 => e.set_layer_opacity(target, 37),
                    1 | 2 => e.toggle_layer_visible(target),
                    _ => e.set_layer_opacity(target, 100),
                }
                let upload = e.take_pending_upload().unwrap();
                assert_eq!(
                    (upload.x, upload.y, upload.width, upload.height),
                    (7, 8, 5, 5),
                    "{mode:?}, group {grouped}"
                );
                check_full_composite_reference(&e);
                assert_eq!(
                    upload.data.as_slice(),
                    e.document.composite.region_rgba(7, 8, 5, 5)
                );
                assert!(e.take_pending_upload().is_none());
            }
            e.set_layer_blend(
                target,
                if mode == rspaint_core::LayerBlend::Overlay {
                    rspaint_core::LayerBlend::Normal
                } else {
                    rspaint_core::LayerBlend::Overlay
                },
            );
            let upload = e.take_pending_upload().unwrap();
            assert_eq!(
                (upload.x, upload.y, upload.width, upload.height),
                (7, 8, 5, 5)
            );
            check_full_composite_reference(&e);
        }
    }
}

#[test]
fn sparse_metadata_bounds_relearn_after_paint_and_merge_pending_updates() {
    let mut e = editor();
    e.add_layer();
    let layer = e.active_layer_id();
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.end_stroke(false);
    drop(e.take_pending_upload());
    e.set_layer_opacity(layer, 50);
    drop(e.take_pending_upload());
    // Keep a stroke in progress: its tree entry is a 1x1 placeholder. The
    // bounds query must use the active working pixels and union pending dirt.
    e.begin_stroke((12, 12), PointerButton::Primary);
    e.set_layer_opacity(layer, 37);
    let upload = e.take_pending_upload().unwrap();
    assert!(upload.x <= 2 && upload.y <= 2);
    assert!(upload.x + upload.width > 12 && upload.y + upload.height > 12);
    check_full_composite_reference(&e);
    e.end_stroke(false);
    drop(e.take_pending_upload());
    // A pending full invalidation still wins over a later regional change.
    e.invalidate_canvas();
    e.toggle_layer_visible(layer);
    let upload = e.take_pending_upload().unwrap();
    assert_eq!(
        (upload.x, upload.y, upload.width, upload.height),
        (0, 0, 16, 16)
    );
    check_full_composite_reference(&e);
}

#[test]
fn empty_layer_metadata_preserves_history_and_other_pending_pixels_without_uploading() {
    let mut e = editor();
    e.add_layer();
    let empty = e.active_layer_id();
    drop(e.take_pending_upload());
    let before = e.history.current();
    e.toggle_layer_visible(empty);
    assert_ne!(e.history.current(), before);
    assert!(e.take_pending_upload().is_none());
    e.set_layer_opacity(empty, 37);
    e.set_layer_blend(empty, rspaint_core::LayerBlend::Multiply);
    assert!(e.take_pending_upload().is_none());
    let base = e.layers()[1].id();
    e.select_layer(base, false, false);
    e.begin_stroke((8, 8), PointerButton::Primary);
    e.end_stroke(false);
    e.toggle_layer_visible(empty);
    assert!(e.take_pending_upload().is_some());
    check_full_composite_reference(&e);
}

#[test]
fn history_upload_reuses_the_already_valid_composite_after_resize_and_jump() {
    fn check(e: &mut Editor, dimensions: (u32, u32)) {
        let mut expected = PixelBuffer::transparent(dimensions.0, dimensions.1);
        rspaint_core::layer::composite_region(
            &e.document.layers,
            e.document.active,
            &e.document.buffer,
            rspaint_core::layer::Region::new(0, 0, dimensions.0, dimensions.1),
            &mut expected,
        );
        // The public restore contract remains eager, before uploading.
        assert_eq!(e.document.composite, expected);
        let ready = e.document.composite.shared_data();
        let upload = e.take_pending_upload().unwrap();
        assert_eq!(
            (upload.x, upload.y, upload.width, upload.height),
            (0, 0, dimensions.0, dimensions.1)
        );
        assert_eq!(upload.data.as_slice(), expected.data());
        // Holding ready would force COW if even a cached recomposition wrote
        // the flattened buffer. Timing-free proof of avoiding that work.
        assert!(std::sync::Arc::ptr_eq(&ready, &upload.data));
        assert!(e.take_pending_upload().is_none());
    }
    let mut e = editor();
    let root = e.history.current();
    e.add_layer();
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.end_stroke(false);
    let painted = e.history.current();
    e.resize_document(23, 19, false);
    drop(e.take_pending_upload());
    e.undo();
    check(&mut e, (16, 16));
    e.redo();
    check(&mut e, (23, 19));
    e.jump_to_history(root);
    check(&mut e, (16, 16));
    // Several restores before any render must upload only the latest state.
    e.jump_to_history(painted);
    e.undo();
    e.redo();
    check(&mut e, (16, 16));
}

#[test]
fn edits_before_a_history_upload_revoke_composite_reuse() {
    for operation in 0..4 {
        let mut e = editor();
        e.add_layer();
        e.begin_stroke((2, 2), PointerButton::Primary);
        e.end_stroke(false);
        e.resize_document(23, 19, false);
        drop(e.take_pending_upload());
        e.undo();
        let ready = e.document.composite.shared_data();
        match operation {
            0 => {
                e.begin_stroke((3, 3), PointerButton::Primary);
                e.end_stroke(false);
            }
            1 => {
                let active = e.active_layer_id();
                e.set_layer_opacity(active, 37);
            }
            2 => e.invalidate_canvas(),
            _ => e.load_buffer(PixelBuffer::new(7, 5, Color::rgb(30, 60, 90)), None),
        }
        let (w, h) = (e.document.width(), e.document.height());
        let mut expected = PixelBuffer::transparent(w, h);
        rspaint_core::layer::composite_region(
            &e.document.layers,
            e.document.active,
            &e.document.buffer,
            rspaint_core::layer::Region::new(0, 0, w, h),
            &mut expected,
        );
        let upload = e.take_pending_upload().unwrap();
        assert_eq!(
            (upload.x, upload.y, upload.width, upload.height),
            (0, 0, w, h)
        );
        assert_eq!(
            upload.data.as_slice(),
            expected.data(),
            "operation {operation}"
        );
        assert!(!std::sync::Arc::ptr_eq(&ready, &upload.data));
        assert!(e.take_pending_upload().is_none());
    }
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

/// A short staircase of pointer segments whose joins create L corners:
/// diagonal, horizontal, vertical, diagonal...
fn pixel_perfect_staircase(e: &mut Editor) {
    e.begin_stroke((2, 2), PointerButton::Primary);
    for p in [(3, 2), (3, 3), (4, 4), (5, 4), (5, 5), (6, 6)] {
        e.update_stroke(p, false);
    }
    e.end_stroke(false);
}

#[test]
fn pixel_perfect_pencil_removes_corner_pixels() {
    let mut e = editor();
    e.set_tool(Tool::Pencil);
    e.settings.pencil_size = 1;
    e.settings.pixel_perfect = true;
    pixel_perfect_staircase(&mut e);
    for (x, y) in [(2, 2), (3, 3), (4, 4), (5, 5), (6, 6)] {
        assert_eq!(
            e.document.buffer.get(x, y),
            Some(Color::BLACK),
            "kept ({x}, {y})"
        );
    }
    for (x, y) in [(3, 2), (5, 4)] {
        assert_eq!(
            e.document.buffer.get(x, y),
            Some(Color::WHITE),
            "corner ({x}, {y})"
        );
    }
}

#[test]
fn pixel_perfect_off_keeps_pencil_corner_pixels() {
    let mut e = editor();
    e.set_tool(Tool::Pencil);
    e.settings.pencil_size = 1;
    e.settings.pixel_perfect = false;
    pixel_perfect_staircase(&mut e);
    for (x, y) in [(3, 2), (5, 4)] {
        assert_eq!(
            e.document.buffer.get(x, y),
            Some(Color::BLACK),
            "corner ({x}, {y})"
        );
    }
}

#[test]
fn pixel_perfect_applies_to_one_pixel_brush() {
    let mut e = editor();
    e.set_tool(Tool::Brush);
    e.settings.brush_size = 1;
    e.settings.pixel_perfect = true;
    pixel_perfect_staircase(&mut e);
    for (x, y) in [(2, 2), (3, 3), (4, 4), (5, 5), (6, 6)] {
        assert_eq!(
            e.document.buffer.get(x, y),
            Some(Color::BLACK),
            "kept ({x}, {y})"
        );
    }
    for (x, y) in [(3, 2), (5, 4)] {
        assert_eq!(
            e.document.buffer.get(x, y),
            Some(Color::WHITE),
            "corner ({x}, {y})"
        );
    }
}

#[test]
fn pixel_perfect_ignores_thicker_pencil_strokes() {
    let draw = |pixel_perfect: bool| {
        let mut e = editor();
        e.set_tool(Tool::Pencil);
        e.settings.pencil_size = 3;
        e.settings.pixel_perfect = pixel_perfect;
        pixel_perfect_staircase(&mut e);
        e.document.buffer
    };
    assert_eq!(draw(true), draw(false));
}

#[test]
fn pixel_perfect_keeps_self_intersection_pixels() {
    let mut e = editor();
    e.set_tool(Tool::Pencil);
    e.settings.pencil_size = 1;
    e.settings.pixel_perfect = true;
    e.begin_stroke((2, 2), PointerButton::Primary);
    for p in [(6, 6), (6, 2), (2, 6)] {
        e.update_stroke(p, false);
    }
    e.end_stroke(false);
    assert_eq!(e.document.buffer.get(4, 4), Some(Color::BLACK));
}

#[test]
fn smoothing_lags_but_ends_at_the_release_point() {
    let mut e = editor();
    e.set_tool(Tool::Pencil);
    e.settings.pencil_size = 1;
    e.settings.stroke_smoothing = 100;
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.update_stroke((12, 2), false);
    let last_before = (2..=12)
        .rev()
        .find(|&x| e.document.buffer.get(x, 2) == Some(Color::BLACK))
        .unwrap();
    assert!(last_before < 12, "painted up to {last_before}");
    e.end_stroke(false);
    for x in 2..=12 {
        assert_eq!(e.document.buffer.get(x, 2), Some(Color::BLACK), "pixel {x}");
    }
}

#[test]
fn zero_smoothing_draws_the_full_segment() {
    let mut e = editor();
    e.set_tool(Tool::Pencil);
    e.settings.pencil_size = 1;
    e.settings.stroke_smoothing = 0;
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.update_stroke((12, 2), false);
    for x in 2..=12 {
        assert_eq!(e.document.buffer.get(x, 2), Some(Color::BLACK), "pixel {x}");
    }
}

#[test]
fn smoothing_applies_to_the_brush_tool() {
    let mut e = editor();
    e.set_tool(Tool::Brush);
    e.settings.brush_size = 3;
    e.settings.stroke_smoothing = 100;
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.update_stroke((12, 2), false);
    assert_eq!(e.document.buffer.get(12, 2), Some(Color::WHITE));
    e.end_stroke(false);
    assert_eq!(e.document.buffer.get(12, 2), Some(Color::BLACK));
}

#[test]
fn smoothing_does_not_affect_shape_tools() {
    let draw = |smoothing: u8| {
        let mut e = editor();
        e.set_tool(Tool::Rectangle);
        e.settings.stroke_size = 1;
        e.settings.stroke_smoothing = smoothing;
        e.begin_stroke((2, 2), PointerButton::Primary);
        e.update_stroke((12, 8), false);
        e.end_stroke(false);
        e.document.buffer
    };
    assert_eq!(draw(0), draw(100));
}

#[test]
fn smoothing_does_not_affect_the_eraser() {
    let draw = |smoothing: u8| {
        let mut e = editor();
        e.document.buffer.fill(Color::BLACK);
        e.set_tool(Tool::Eraser);
        e.settings.eraser_size = 3;
        e.settings.stroke_smoothing = smoothing;
        e.begin_stroke((2, 2), PointerButton::Primary);
        e.update_stroke((12, 2), false);
        e.end_stroke(false);
        e.document.buffer
    };
    assert_eq!(draw(0), draw(100));
}

#[test]
fn end_stroke_at_drains_to_the_release_point() {
    let mut e = editor();
    e.set_tool(Tool::Pencil);
    e.settings.pencil_size = 1;
    e.settings.stroke_smoothing = 100;
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.update_stroke((8, 2), false);
    e.end_stroke_at((14, 2), false);
    for x in 2..=14 {
        assert_eq!(e.document.buffer.get(x, 2), Some(Color::BLACK), "pixel {x}");
    }
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
fn moving_one_area_of_a_multiselection_keeps_the_transform_box_tight() {
    let mut e = editor();
    select(&mut e, (1, 1), (2, 2));
    e.begin_selection_drag((8, 8), Tool::Select, true, false, false);
    e.update_selection_drag((9, 9));
    e.end_selection_drag();
    assert_eq!(e.selection_bounds(), Some((1, 1, 9, 9)));

    // Drag only the top-left area (outline only, without Ctrl).
    e.begin_selection_drag((1, 1), Tool::Select, false, false, false);
    e.update_selection_drag((1, 6));
    e.end_selection_drag();

    // The box now wraps just the two areas: (1,6)-(2,7) and (8,8)-(9,9).
    assert_eq!(e.selection_bounds(), Some((1, 6, 9, 4)));
    assert!(e.point_in_selection((1, 6)));
    assert!(e.point_in_selection((8, 8)));
    assert!(!e.point_in_selection((1, 1)));
}

#[test]
fn pasted_selection_keeps_transform_mode_until_it_is_dropped() {
    let mut e = editor();
    select(&mut e, (1, 1), (2, 2));
    assert!(!e.selection_is_pasted());
    let pixels = e.copy_selection().unwrap();

    e.paste_clipboard(Some(pixels.clone()));
    assert!(e.selection_is_pasted());
    // Dropping it (a press outside) commits it and ends the mode.
    e.deselect();
    assert!(e.selection.is_none());
    assert!(!e.selection_is_pasted());

    // A drawing tool settles it into an ordinary selection.
    e.paste_clipboard(Some(pixels));
    assert!(e.selection_is_pasted());
    e.set_tool(Tool::Pencil);
    assert!(e.selection.is_some());
    assert!(!e.selection_is_pasted());
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

fn layer_ids(e: &Editor) -> Vec<u64> {
    e.layers().iter().map(|node| node.id()).collect()
}

#[test]
fn curve_previews_restore_old_pixels_and_upload_only_the_affected_region() {
    let mut e = editor();
    e.load_buffer(PixelBuffer::new(128, 128, Color::WHITE), None);
    e.settings.stroke_size = 3;
    e.take_pending_upload();
    let base = e.document.buffer.clone();
    e.curve_pointer_down((20, 20));
    e.curve_pointer_move((50, 50));
    e.take_pending_upload();
    e.curve_pointer_move((30, 20));
    assert_eq!(e.document.buffer.get(49, 49), base.get(49, 49));
    let upload = e.take_pending_upload().unwrap();
    assert!(upload.width < 128 && upload.height < 128);
    e.curve_pointer_up();
    e.curve_pointer_down((25, 45));
    e.curve_pointer_move((25, 35));
    e.curve_pointer_up();
    e.curve_pointer_down((35, 40));
    e.curve_pointer_move((40, 30));
    let brush = rspaint_core::Brush::new(rspaint_core::BrushShape::Square, 3)
        .with_blend(e.settings.paint_blend);
    let mut expected = base.clone();
    rspaint_core::draw::draw_bezier(
        &mut expected,
        (20, 20),
        (25, 35),
        (40, 30),
        (30, 20),
        &brush,
        e.colors.foreground,
    );
    assert_eq!(e.document.buffer, expected);
    e.take_pending_upload();
    e.curve_cancel();
    assert_eq!(e.document.buffer, base);
    let upload = e.take_pending_upload().unwrap();
    assert!(upload.width < 128 && upload.height < 128);
    assert_eq!(e.document.composite, base);
}

#[test]
fn layer_metadata_and_activation_do_not_upload_unchanged_pixels() {
    let mut e = editor();
    let background = e.active_layer_id();
    e.add_layer();
    e.take_pending_upload();
    e.rename_layer(e.active_layer_id(), "Renamed");
    assert!(e.take_pending_upload().is_none());
    e.select_layer(background, false, false);
    assert!(e.take_pending_upload().is_none());
    e.begin_stroke((8, 8), PointerButton::Primary);
    e.end_stroke(false);
    assert!(e.take_pending_upload().is_some());
}

#[test]
fn composition_cache_follows_undo_redo_branches_and_resize() {
    fn check(e: &mut Editor) {
        e.take_pending_upload();
        let mut expected = PixelBuffer::transparent(e.document.width(), e.document.height());
        rspaint_core::layer::composite_region(
            &e.document.layers,
            e.document.active,
            &e.document.buffer,
            rspaint_core::layer::Region::new(0, 0, e.document.width(), e.document.height()),
            &mut expected,
        );
        assert_eq!(e.document.composite, expected);
    }
    let mut e = editor();
    e.load_buffer(PixelBuffer::new(130, 70, Color::WHITE), None);
    e.add_layer();
    e.begin_stroke((65, 32), PointerButton::Primary);
    e.end_stroke(false);
    check(&mut e);
    e.undo();
    check(&mut e);
    e.redo();
    check(&mut e);
    e.undo();
    e.colors.foreground = Color::rgb(50, 100, 150);
    e.begin_stroke((64, 33), PointerButton::Primary);
    e.end_stroke(false);
    check(&mut e);
    e.resize_document(67, 33, true);
    check(&mut e);
    e.undo();
    check(&mut e);
    e.resize_document(194, 129, true);
    check(&mut e);
}

#[test]
fn block_history_preserves_sibling_branches_and_large_transforms() {
    let mut e = editor();
    let mut original = PixelBuffer::transparent(259, 131);
    original.set(127, 127, Color::BLACK);
    original.set(258, 130, Color::rgba(30, 60, 90, 120));
    e.load_buffer(original.clone(), None);
    let root = e.history.current();
    e.document.buffer.set(128, 128, Color::BLACK);
    e.commit("First pixel");
    let first_id = e.history.current();
    let first = e.document.buffer.clone();
    e.document.buffer.set(256, 130, Color::BLACK);
    e.commit("Second pixel");
    let second_id = e.history.current();
    let second = e.document.buffer.clone();
    e.undo();
    assert_eq!(e.document.buffer, first);
    e.document.buffer.set(0, 0, Color::BLACK);
    e.commit("Branch");
    let branch_id = e.history.current();
    let branch = e.document.buffer.clone();
    assert_eq!(e.history.node(first_id).unwrap().children.len(), 2);
    e.jump_to_history(second_id);
    assert_eq!(e.document.buffer, second);
    e.jump_to_history(branch_id);
    assert_eq!(e.document.buffer, branch);
    e.rename_layer(e.active_layer_id(), "Renamed");
    e.undo();
    assert_eq!(e.document.buffer, branch);
    e.resize_document(131, 259, true);
    let resized = e.document.buffer.clone();
    e.undo();
    assert_eq!(e.document.buffer, branch);
    e.redo();
    assert_eq!(e.document.buffer, resized);
    e.jump_to_history(root);
    assert_eq!(e.document.buffer, original);
    e.jump_to_history(second_id);
    assert_eq!(e.document.buffer, second);
}

#[test]
fn local_previews_preserve_working_allocation_and_cancel_exactly() {
    let mut e = editor();
    e.load_buffer(PixelBuffer::new(513, 259, Color::WHITE), None);
    drop(e.take_pending_upload());
    for tool in [
        Tool::Pencil,
        Tool::Brush,
        Tool::Eraser,
        Tool::Line,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::RoundedRectangle,
        Tool::Curve,
        Tool::Text,
    ] {
        e.set_tool(tool);
        let address = e.document.buffer.data().as_ptr();
        match tool {
            Tool::Curve => {
                e.curve_pointer_down((127, 127));
                e.curve_pointer_move((258, 258));
                e.curve_pointer_up();
                e.curve_pointer_down((-5, 130));
                e.curve_pointer_move((200, 100));
            }
            Tool::Text => {
                e.begin_text_box((127, 127));
                e.update_text_box((258, 190));
                e.finish_text_box();
                e.text_type("Hola");
                e.render_text_preview();
                e.text_backspace();
                e.render_text_preview();
            }
            _ => {
                e.begin_stroke((127, 127), PointerButton::Primary);
                e.update_stroke((258, 258), false);
                e.update_stroke((129, 130), false);
            }
        }
        assert_eq!(address, e.document.buffer.data().as_ptr(), "{tool:?}");
        match tool {
            Tool::Curve => e.curve_cancel(),
            Tool::Text => e.cancel_text(),
            _ => e.cancel_stroke(),
        }
        assert_eq!(
            e.document.buffer,
            PixelBuffer::new(513, 259, Color::WHITE),
            "cancel {tool:?}"
        );
        assert_eq!(
            e.layer_pixels(e.active_layer_id()).unwrap().size(),
            [513, 259]
        );
    }
    // A genuine external snapshot must retain normal copy-on-write isolation.
    let external = e.document.buffer.clone();
    e.set_tool(Tool::Pencil);
    e.begin_stroke((128, 128), PointerButton::Primary);
    assert_eq!(external.get(128, 128), Some(Color::WHITE));
    assert_eq!(e.document.buffer.get(128, 128), Some(Color::BLACK));
    e.cancel_stroke();
    assert_eq!(external, e.document.buffer);
}

#[test]
fn local_curve_and_text_backups_commit_and_restore_through_history() {
    let mut e = editor();
    e.load_buffer(PixelBuffer::new(259, 131, Color::WHITE), None);
    e.set_tool(Tool::Curve);
    e.curve_pointer_down((127, 30));
    e.curve_pointer_move((258, 130));
    e.curve_pointer_up();
    e.curve_pointer_down((200, 0));
    e.curve_pointer_up();
    e.curve_pointer_down((140, 100));
    e.curve_pointer_up();
    let curve = e.document.buffer.clone();
    e.undo();
    assert_eq!(e.document.buffer, PixelBuffer::new(259, 131, Color::WHITE));
    e.redo();
    assert_eq!(e.document.buffer, curve);
    e.set_tool(Tool::Text);
    e.begin_text_box((120, 80));
    e.update_text_box((250, 130));
    e.finish_text_box();
    e.text_type("Hola");
    e.render_text_preview();
    e.commit_text();
    let text = e.document.buffer.clone();
    assert_ne!(curve, text);
    assert_eq!(
        e.layer_node(e.active_layer_id()).unwrap().pixel_size(),
        Some((259, 131))
    );
    e.undo();
    assert_eq!(e.document.buffer, curve);
    e.redo();
    assert_eq!(e.document.buffer, text);
}

#[test]
fn regional_curve_preview_preserves_selection_clipping_and_canvas_edges() {
    let mut e = editor();
    select(&mut e, (2, 2), (8, 8));
    e.set_tool(Tool::Curve);
    e.settings.stroke_size = 5;
    e.take_pending_upload();
    let base = e.document.buffer.clone();
    e.curve_pointer_down((-4, 4));
    e.curve_pointer_move((20, 4));
    e.take_pending_upload();
    e.curve_pointer_move((6, 4));
    assert_eq!(e.document.buffer.get(4, 4), Some(Color::BLACK));
    for y in 0..16 {
        for x in 0..16 {
            if !(2..=8).contains(&x) || !(2..=8).contains(&y) {
                assert_eq!(e.document.buffer.get(x, y), base.get(x, y));
            }
        }
    }
    e.curve_cancel();
    assert_eq!(e.document.buffer, base);
}

fn layer_pixel(e: &Editor, index: usize, x: u32, y: u32) -> Option<Color> {
    e.layers()[index].layer()?.pixels.get(x, y)
}

#[test]
fn new_document_starts_with_one_background_layer() {
    let e = editor();
    assert_eq!(e.layers().len(), 1);
    assert_eq!(e.layers()[0].name(), "Background");
    assert_eq!(e.active_layer_id(), e.layers()[0].id());
    assert_eq!(e.document.composite.get(0, 0), Some(Color::WHITE));
}

#[test]
fn added_layers_are_transparent_and_paint_in_front() {
    let mut e = editor();
    e.add_layer();
    assert_eq!(e.layers().len(), 2);
    let front = e.layers()[0].id();
    assert_eq!(e.active_layer_id(), front);
    assert_eq!(layer_pixel(&e, 0, 2, 2), Some(Color::TRANSPARENT));

    e.set_tool(Tool::Pencil);
    e.settings.pencil_size = 1;
    e.colors.foreground = Color::BLACK;
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.end_stroke(false);
    e.document.composite_all();
    assert_eq!(e.document.composite.get(2, 2), Some(Color::BLACK));
    // The background layer underneath is untouched.
    assert_eq!(layer_pixel(&e, 1, 2, 2), Some(Color::WHITE));
}

#[test]
fn visibility_opacity_and_order_change_the_composite() {
    let mut e = editor();
    e.add_layer();
    let top = e.layers()[0].id();
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.end_stroke(false);
    e.document.composite_all();
    assert_eq!(e.document.composite.get(2, 2), Some(Color::BLACK));

    e.set_layer_opacity(top, 50);
    e.document.composite_all();
    assert_eq!(
        e.document.composite.get(2, 2),
        Some(Color::rgb(128, 128, 128))
    );

    e.toggle_layer_visible(top);
    e.document.composite_all();
    assert_eq!(e.document.composite.get(2, 2), Some(Color::WHITE));
    assert!(!e.can_paint_active());

    e.toggle_layer_visible(top);
    e.move_layer_after(top, e.layers()[1].id());
    e.document.composite_all();
    assert_eq!(e.document.composite.get(2, 2), Some(Color::WHITE));
}

#[test]
fn groups_apply_their_opacity_to_all_children() {
    let mut e = editor();
    e.add_group();
    let group = e.active_layer_id();
    assert!(e.layers()[0].is_group());
    e.add_layer();
    let child = e.active_layer_id();
    e.begin_stroke((3, 3), PointerButton::Primary);
    e.end_stroke(false);
    e.document.composite_all();
    assert_eq!(e.document.composite.get(3, 3), Some(Color::BLACK));

    e.set_layer_opacity(group, 50);
    e.document.composite_all();
    assert_eq!(
        e.document.composite.get(3, 3),
        Some(Color::rgb(128, 128, 128))
    );

    e.toggle_layer_visible(group);
    e.document.composite_all();
    assert_eq!(e.document.composite.get(3, 3), Some(Color::WHITE));
    // The child is not editable while its group is hidden.
    e.select_layer(child, false, false);
    assert!(!e.can_paint_active());
}

#[test]
fn multi_selection_keeps_the_first_click_active_and_merges() {
    let mut e = editor();
    e.add_layer();
    let first = e.active_layer_id();
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.end_stroke(false);
    e.add_layer();
    let second = e.active_layer_id();
    e.colors.foreground = Color::rgb(200, 20, 20);
    e.begin_stroke((6, 6), PointerButton::Primary);
    e.end_stroke(false);

    e.select_layer(first, false, true);
    assert_eq!(e.active_layer_id(), second);
    assert_eq!(e.layer_selection().len(), 2);

    e.merge_selected_layers();
    assert_eq!(e.layers().len(), 2);
    e.document.composite_all();
    assert_eq!(e.document.composite.get(2, 2), Some(Color::BLACK));
    assert_eq!(
        e.document.composite.get(6, 6),
        Some(Color::rgb(200, 20, 20))
    );

    e.undo();
    assert_eq!(e.layers().len(), 3);
}

#[test]
fn locked_layers_cannot_be_painted() {
    let mut e = editor();
    e.add_layer();
    let top = e.active_layer_id();
    e.toggle_layer_locked(top);
    assert!(!e.can_paint_active());
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.end_stroke(false);
    e.document.composite_all();
    assert_eq!(e.document.composite.get(2, 2), Some(Color::WHITE));
    e.toggle_layer_locked(top);
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.end_stroke(false);
    e.document.composite_all();
    assert_eq!(e.document.composite.get(2, 2), Some(Color::BLACK));
}

#[test]
fn adding_a_layer_is_undoable_and_redoable() {
    let mut e = editor();
    e.add_layer();
    assert_eq!(e.layers().len(), 2);
    e.undo();
    assert_eq!(e.layers().len(), 1);
    assert_eq!(e.active_layer_id(), e.layers()[0].id());
    e.redo();
    assert_eq!(e.layers().len(), 2);
}

#[test]
fn canvas_resize_applies_to_every_layer() {
    let mut e = editor();
    e.add_layer();
    e.resize_document(20, 12, false);
    assert_eq!(e.document.width(), 20);
    assert_eq!(e.document.height(), 12);
    for node in e.layers() {
        let pixels = &node.layer().unwrap().pixels;
        assert_eq!((pixels.width(), pixels.height()), (20, 12));
    }
    assert_eq!(
        (e.document.composite.width(), e.document.composite.height()),
        (20, 12)
    );
}

#[test]
fn color_picker_samples_the_composite() {
    let mut e = editor();
    e.add_layer();
    e.begin_stroke((4, 4), PointerButton::Primary);
    e.end_stroke(false);
    e.set_tool(Tool::PickColor);
    e.click_tool((4, 4), PointerButton::Primary, false);
    assert_eq!(e.colors.foreground, Color::BLACK);
}

#[test]
fn rename_and_group_expansion_are_public_state() {
    let mut e = editor();
    let background = e.active_layer_id();
    e.rename_layer(background, "Base");
    assert_eq!(e.layers()[0].name(), "Base");
    e.add_group();
    let group = e.active_layer_id();
    assert!(e.layer_node(group).unwrap().expanded());
    e.toggle_group_expanded(group);
    assert!(!e.layer_node(group).unwrap().expanded());
    assert_eq!(layer_ids(&e).len(), 2);
}

#[test]
fn uploads_flatten_the_layer_stack() {
    let mut e = editor();
    e.begin_stroke((2, 2), PointerButton::Primary);
    e.end_stroke(false);
    // A new transparent layer becomes active; the canvas must still show the
    // black pixel painted on the background underneath.
    e.add_layer();
    let upload = e.take_pending_upload().unwrap();
    assert_eq!(upload.width, 16);
    let index = ((2 * 16 + 2) * 4) as usize;
    assert_eq!(&upload.data[index..index + 4], &[0, 0, 0, 255]);
}

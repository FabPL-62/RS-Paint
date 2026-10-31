//! cargo run --release -p rspaint-editor --example sparse_layer_changes_bench --offline
//! CPU action + compose/stage; no native input scheduling, GPU or display.
use rspaint_core::{
    layer::{composite_region, Region},
    Color, Layer, LayerGroup, LayerNode, PixelBuffer,
};
use rspaint_editor::{Editor, PointerButton};
use std::time::Instant;

fn measure(e: &mut Editor, action: impl FnOnce(&mut Editor)) -> (f64, usize) {
    let start = Instant::now();
    action(e);
    let uploads = e.take_pending_uploads();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    (ms, uploads.iter().map(|u| u.data.len()).sum())
}

fn main() {
    let after_paint = std::env::args().any(|arg| arg == "--after-paint");
    let separated = std::env::args().any(|arg| arg == "--separated");
    if separated {
        println!("Separated content: two 120x80 patches near opposite corners");
    }
    if after_paint {
        println!("Each opacity event includes a one-pixel stroke and history commit before the metadata change");
    }
    for (w, h) in [(1920, 1080), (6000, 4000)] {
        for kind in ["above", "below", "group", "empty"] {
            let mut e = Editor::new();
            e.load_buffer(PixelBuffer::new(w, h, Color::WHITE), None);
            let mut source = PixelBuffer::transparent(w, h);
            let mut seed = 0x12345678u32;
            for p in source.data_mut().chunks_exact_mut(4) {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                p.copy_from_slice(&[seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 140]);
            }
            e.document.layers.splice(
                0..0,
                (100..106).map(|id| LayerNode::Layer(Layer::new(id, "Static", source.clone()))),
            );
            drop(source);
            let mut sparse = PixelBuffer::transparent(w, h);
            for y in 300..if kind == "empty" { 300 } else { 380 } {
                for p in sparse.row_mut(y, 300, 120).chunks_exact_mut(4) {
                    p.copy_from_slice(&[20, 90, 160, 180]);
                }
            }
            if separated && kind != "empty" {
                for y in h - 380..h - 300 {
                    for p in sparse.row_mut(y, w - 420, 120).chunks_exact_mut(4) {
                        p.copy_from_slice(&[160, 40, 80, 180]);
                    }
                }
            }
            let layer = LayerNode::Layer(Layer::new(200, "Sparse", sparse));
            let target;
            if kind == "group" {
                let mut group = LayerGroup::new(201, "Sparse group");
                group.children.push(layer);
                e.document.layers.insert(0, LayerNode::Group(group));
                target = 201;
            } else {
                let index = if kind == "below" {
                    e.document.layers.len() - 1
                } else {
                    0
                };
                e.document.layers.insert(index, layer);
                target = 200;
            }
            e.document.next_id = 202;
            e.select_layer(200, false, false);
            e.commit("Fixture");
            e.invalidate_canvas();
            drop(e.take_pending_upload());
            let cold = measure(&mut e, |e| e.set_layer_opacity(target, 51));
            let mut opacity = Vec::new();
            let mut max_bytes = 0;
            for value in 53..=59 {
                let (ms, bytes) = measure(&mut e, |e| {
                    if after_paint {
                        e.begin_stroke((350 + (value - 53) as i32, 350), PointerButton::Primary);
                        e.end_stroke(false);
                    }
                    e.set_layer_opacity(target, value);
                });
                opacity.push(ms);
                max_bytes = max_bytes.max(bytes);
            }
            let mut visibility = Vec::new();
            for _ in 0..6 {
                let (ms, bytes) = measure(&mut e, |e| e.toggle_layer_visible(target));
                visibility.push(ms);
                max_bytes = max_bytes.max(bytes);
            }
            opacity.sort_by(f64::total_cmp);
            visibility.sort_by(f64::total_cmp);
            let ink = if kind == "empty" {
                "empty"
            } else if separated {
                "2x120x80"
            } else {
                "120x80"
            };
            println!("{w}x{h} {kind}, 8 layers, ink{ink}: cold={:.3}ms {}B; opacity median={:.3}ms max={:.3}ms; visibility median={:.3}ms max={:.3}ms; max_upload={max_bytes}B", cold.0, cold.1, opacity[3], opacity[6], visibility[3], visibility[5]);
            let mut expected = PixelBuffer::transparent(w, h);
            composite_region(
                &e.document.layers,
                e.document.active,
                &e.document.buffer,
                Region::new(0, 0, w, h),
                &mut expected,
            );
            assert_eq!(e.document.composite, expected, "{kind}");
            println!("  complete reference passed");
        }
    }
}

//! Run: cargo run --release -p rspaint-editor --example curve_layers_bench
//! CPU-only timings; excludes GPU upload. Reports medians after warm-up.
use rspaint_core::{draw, Brush, BrushShape, Color, PixelBuffer};
use rspaint_editor::Editor;
use std::{hint::black_box, time::Instant};

fn median(mut f: impl FnMut()) -> f64 {
    f();
    let mut samples = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        f();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    samples[3]
}

fn main() {
    for (width, height) in [(1920, 1080), (6000, 4000)] {
        for layers in [1, 8] {
            let mut e = Editor::new();
            let base = PixelBuffer::new(width, height, Color::WHITE);
            e.load_buffer(base.clone(), None);
            for _ in 1..layers {
                e.add_layer();
                // Nonempty semitransparent layers exercise actual blending.
                e.document.buffer = PixelBuffer::new(width, height, Color::rgba(40, 90, 150, 128));
            }
            e.document.sync_active();
            drop(e.take_pending_upload());
            let full = median(|| e.document.composite_all());
            let region = median(|| e.document.composite_rect(100, 100, 256, 256));
            // First write to a shared snapshot versus subsequent exclusive writes.
            let cow = median(|| {
                let mut snapshot = base.clone();
                snapshot.set(100, 100, Color::BLACK);
                black_box(snapshot);
            });
            e.settings.stroke_size = 3;
            e.curve_pointer_down((100, 100));
            e.curve_pointer_move((300, 200));
            e.curve_pointer_up();
            e.curve_pointer_down((150, 250));
            drop(e.take_pending_upload());
            let mut tick = 0;
            let curve = median(|| {
                tick ^= 1;
                e.curve_pointer_move((150 + tick, 250));
                black_box(e.take_pending_upload());
            });
            // Previous preview strategy: full restore, rasterize, full compose.
            let saved = e.document.buffer.clone();
            let brush = Brush::new(BrushShape::Square, 3).with_blend(e.settings.paint_blend);
            let old_curve = median(|| {
                e.document
                    .buffer
                    .copy_region_from(&saved, 0, 0, width, height);
                draw::draw_quadratic_curve(
                    &mut e.document.buffer,
                    (100, 100),
                    (150, 250),
                    (300, 200),
                    &brush,
                    Color::BLACK,
                );
                e.document.composite_all();
                black_box(e.document.composite.shared_data());
            });
            println!("{width}x{height}, {layers} layers: full={full:.3}ms region256={region:.3}ms first-write={cow:.3}ms curve-old={old_curve:.3}ms curve-regional={curve:.3}ms");
        }
    }
}

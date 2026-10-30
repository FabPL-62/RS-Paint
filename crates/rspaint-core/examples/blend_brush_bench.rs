//! cargo run --release -p rspaint-core --example blend_brush_bench --offline
//! CPU drawing only; excludes initial COW, mask setup and GPU composition.
use rspaint_core::{draw, Brush, BrushShape, Color, PixelBuffer};
use std::{hint::black_box, time::Instant};

fn main() {
    let base = PixelBuffer::new(1920, 1080, Color::rgba(30, 80, 140, 150));
    let color = Color::rgba(210, 70, 20, 96);
    for shape in [BrushShape::Square, BrushShape::Circle] {
        for size in [8, 64, 256] {
            let brush = Brush::new(shape, size);
            let mut old_times = Vec::new();
            let mut new_times = Vec::new();
            for _ in 0..3 {
                let mut old = base.clone();
                let mut new = base.clone();
                black_box(old.data_mut());
                black_box(new.data_mut());
                let mut mask: Vec<_> = (0..1920 * 1080).map(|i| i % 17 == 0).collect();
                let mut old_mask = mask.clone();
                let start = Instant::now();
                draw::walk_dense_line((300, 300), (1300, 800), |x, y| {
                    brush.stamp_masked(&mut old, &mut old_mask, 1920, x, y, color)
                });
                old_times.push(start.elapsed().as_secs_f64() * 1000.0);
                let start = Instant::now();
                draw::draw_line_masked(
                    &mut new,
                    (300, 300),
                    (1300, 800),
                    &brush,
                    color,
                    &mut mask,
                    1920,
                );
                new_times.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(black_box(&new), black_box(&old));
                assert_eq!(mask, old_mask);
            }
            old_times.sort_by(f64::total_cmp);
            new_times.sort_by(f64::total_cmp);
            println!("1920x1080, {shape:?} size {size}, alpha96: stamps={:.3}ms spans={:.3}ms ({:.1}x), median of 3",old_times[1],new_times[1],old_times[1]/new_times[1]);
        }
    }
}

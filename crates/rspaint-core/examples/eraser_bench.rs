//! Reproducible CPU comparison with the previous per-stamp color eraser.
//! Run: cargo run --release -p rspaint-core --example eraser_bench
use rspaint_core::{draw, Brush, BrushShape, Color, PixelBuffer};
use std::{hint::black_box, time::Instant};

fn main() {
    let base = PixelBuffer::new(6000, 4000, Color::BLACK);
    // At 12.5% zoom this is a 125 x 62.5 screen-pixel pointer movement.
    let a = (2000, 1500);
    let b = (3000, 2000);
    for size in [8, 64, 256] {
        let brush = Brush::new(BrushShape::Square, size);
        let mut reference = base.clone();
        let start = Instant::now();
        draw::walk_dense_line(a, b, |x, y| {
            brush.replace_in_stamp(&mut reference, x, y, Color::BLACK, Color::WHITE);
        });
        let old = start.elapsed();
        let mut optimized = base.clone();
        let start = Instant::now();
        draw::erase_line(&mut optimized, a, b, &brush, Color::BLACK, Color::WHITE);
        let new = start.elapsed();
        assert_eq!(black_box(&optimized), black_box(&reference));
        println!(
            "6000x4000, size {size}: stamps {old:?}, spans {new:?}, {:.1}x",
            old.as_secs_f64() / new.as_secs_f64()
        );
        // Continue the same stroke: pixels are now exclusively owned, so these
        // timings isolate the drawing work from the first copy-on-write.
        let next_a = (3000, 2000);
        let next_b = (4000, 2500);
        let start = Instant::now();
        draw::walk_dense_line(next_a, next_b, |x, y| {
            brush.replace_in_stamp(&mut reference, x, y, Color::BLACK, Color::WHITE);
        });
        let old = start.elapsed();
        let start = Instant::now();
        draw::erase_line(
            &mut optimized,
            next_a,
            next_b,
            &brush,
            Color::BLACK,
            Color::WHITE,
        );
        let new = start.elapsed();
        assert_eq!(black_box(&optimized), black_box(&reference));
        println!(
            "  continuing stroke: stamps {old:?}, spans {new:?}, {:.1}x",
            old.as_secs_f64() / new.as_secs_f64()
        );
    }
}

//! CPU update timings for one long, uninterrupted stroke on a large image.
//! cargo run --release -p rspaint-editor --example selection_brush_bench
use rspaint_core::{BrushShape, Color, PixelBuffer};
use rspaint_editor::Editor;
use std::{hint::black_box, time::Instant};

fn main() {
    let mut editor = Editor::new();
    editor.load_buffer(PixelBuffer::new(6000, 4000, Color::WHITE), None);
    editor.settings.brush_shape = BrushShape::Square;
    editor.settings.brush_size = 128;
    let start = Instant::now();
    editor.selection_brush_begin((100, 100), false, false);
    println!("6000x4000, brush 128: begin {:?}", start.elapsed());
    let mut elapsed = Vec::new();
    // Sweep across the image repeatedly without releasing the button. Each
    // update moves 200 canvas pixels (25 screen pixels at 12.5% zoom).
    for row in 0..20 {
        for step in 0..30 {
            let x = if row % 2 == 0 {
                100 + step * 195
            } else {
                5755 - step * 195
            };
            let start = Instant::now();
            editor.selection_brush_to((x, 100 + row * 180));
            elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    for (label, samples) in [
        ("first 30", &elapsed[..30]),
        ("last 30", &elapsed[elapsed.len() - 30..]),
    ] {
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        println!("{label} updates: mean {mean:.3} ms");
    }
    let tiles = black_box(editor.selection_brush_tiles().unwrap());
    let runs: usize = tiles.iter().map(|t| t.runs.len()).sum();
    let rects: usize = tiles.iter().map(|t| t.highlight_rects.len()).sum();
    println!("highlight geometry: {runs} pixel rows -> {rects} cached rectangles");
}

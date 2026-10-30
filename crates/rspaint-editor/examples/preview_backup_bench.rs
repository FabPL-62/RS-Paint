//! cargo run --release -p rspaint-editor --example preview_backup_bench --offline
//! CPU-only. Shared first-write is a reconstructed COW cost, not the entire
//! former tool path. Actual starts include local backup/mask/raster work.
use rspaint_core::{Color, PixelBuffer};
use rspaint_editor::{tools::Tool, Editor, PointerButton};
use std::time::Instant;

fn main() {
    for (width, height) in [(1920, 1080), (6000, 4000)] {
        let mut e = Editor::new();
        e.load_buffer(PixelBuffer::new(width, height, Color::WHITE), None);
        // Display the initial canvas first, as in normal interactive use.
        drop(e.take_pending_upload());
        let mut old = Vec::new();
        for _ in 0..7 {
            let mut shared = e.document.buffer.clone();
            let start = Instant::now();
            shared.set(127, 127, Color::BLACK);
            old.push(start.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(shared);
        }
        old.sort_by(f64::total_cmp);
        println!(
            "{width}x{height}: reconstructed shared first-write median={:.3}ms",
            old[3]
        );
        for tool in [Tool::Pencil, Tool::Line, Tool::Curve, Tool::Text] {
            e.set_tool(tool);
            let mut samples = Vec::new();
            for _ in 0..7 {
                let address = e.document.buffer.data().as_ptr();
                let start = Instant::now();
                match tool {
                    Tool::Curve => {
                        e.curve_pointer_down((127, 127));
                        e.curve_pointer_move((160, 150));
                    }
                    Tool::Text => {
                        e.begin_text_box((127, 127));
                        e.update_text_box((250, 190));
                        e.finish_text_box();
                        e.text_type("Hola");
                        e.render_text_preview();
                    }
                    _ => e.begin_stroke((127, 127), PointerButton::Primary),
                }
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(
                    address,
                    e.document.buffer.data().as_ptr(),
                    "full working buffer copied: {tool:?}"
                );
                match tool {
                    Tool::Curve => e.curve_cancel(),
                    Tool::Text => e.cancel_text(),
                    _ => e.cancel_stroke(),
                }
            }
            samples.sort_by(f64::total_cmp);
            println!("  {tool:?}: start/preview median={:.3}ms max={:.3}ms; working allocation preserved",samples[3],samples[6]);
        }
    }
}

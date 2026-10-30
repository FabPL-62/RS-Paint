//! Run: cargo run --release -p rspaint-editor --example history_memory_bench
//! Counts unique RGBA allocations retained by history, excluding metadata,
//! allocator overhead, GPU textures, and temporary/composite images.
use rspaint_core::{snapshot::SnapshotNode, Color, PixelBuffer};
use rspaint_editor::Editor;
use std::{collections::HashMap, time::Instant};

fn collect(nodes: &[SnapshotNode], buffers: &mut HashMap<usize, usize>) {
    for node in nodes {
        if let Some(pixels) = node.layer_pixels() {
            for bytes in pixels.allocations() {
                buffers.insert(bytes.as_ptr() as usize, bytes.len());
            }
        }
        collect(node.children(), buffers);
    }
}

fn retained(e: &Editor) -> usize {
    let mut buffers = HashMap::new();
    for node in e.history.nodes() {
        collect(&node.snapshot.layers, &mut buffers);
    }
    buffers.values().sum()
}

fn run(width: u32, height: u32) {
    let mut e = Editor::new();
    e.load_buffer(PixelBuffer::new(width, height, Color::WHITE), None);
    let initial = retained(&e);
    let mut samples = Vec::new();
    let mut commit_samples = Vec::new();
    for x in 0..12 {
        let start = Instant::now();
        e.document.buffer.set(x, 0, Color::BLACK);
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        e.commit("Pixel");
        commit_samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let edited = retained(&e);
    for n in 0..12 {
        e.rename_layer(e.active_layer_id(), &format!("Name {n}"));
    }
    let renamed = retained(&e);
    // Undo/redo and a new branch retain every original pixel snapshot.
    for _ in 0..13 {
        e.undo();
    }
    assert_eq!(e.document.buffer.get(11, 0), Some(Color::WHITE));
    e.redo();
    assert_eq!(e.document.buffer.get(11, 0), Some(Color::BLACK));
    e.undo();
    e.document.buffer.set(20, 0, Color::BLACK);
    e.commit("Branch pixel");
    samples.sort_by(f64::total_cmp);
    commit_samples.sort_by(f64::total_cmp);
    println!("{width}x{height}, retained RGBA: initial={:.3}MiB, 12 pixel edits={:.3}MiB, 12 renames={:.3}MiB, branch={:.3}MiB; first-write median={:.3}ms max={:.3}ms",
        initial as f64 / 1048576.0, edited as f64 / 1048576.0, renamed as f64 / 1048576.0, retained(&e) as f64 / 1048576.0,
        samples[6], samples[11]);
    println!(
        "snapshot commit median={:.3}ms max={:.3}ms; edit growth={} bytes (12 blocks)",
        commit_samples[6],
        commit_samples[11],
        edited - initial
    );
    assert_eq!(renamed, edited, "metadata must share pixel allocations");
    assert_eq!(edited - initial, 12 * 128 * 128 * 4);
}

fn main() {
    for (width, height) in [(2048, 2048), (6000, 4000)] {
        run(width, height);
    }
}

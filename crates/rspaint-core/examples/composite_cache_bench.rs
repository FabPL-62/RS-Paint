//! cargo run --release -p rspaint-core --example composite_cache_bench --offline
//! CPU-only. Uncached reference uses the same optimized blend kernel.
use rspaint_core::{
    layer::{composite_region, Region},
    Color, Document, Layer, LayerGroup, LayerNode, PixelBuffer,
};
use std::{hint::black_box, time::Instant};

fn main() {
    for (width, height) in [(1920, 1080), (6000, 4000)] {
        for grouped in [false, true] {
            let mut doc = Document::new(width, height);
            let source = PixelBuffer::new(width, height, Color::rgba(50, 110, 170, 128));
            let static_layers: Vec<_> = (2..8)
                .map(|id| LayerNode::Layer(Layer::new(id, "Static", source.clone())))
                .collect();
            if grouped {
                let mut group = LayerGroup::new(20, "Static group");
                group.opacity = 75;
                group.children = static_layers;
                doc.layers.insert(0, LayerNode::Group(group));
            } else {
                doc.layers.splice(0..0, static_layers);
            }
            doc.active = 8;
            doc.buffer = PixelBuffer::new(width, height, Color::rgba(150, 60, 20, 100));
            doc.layers.insert(
                0,
                LayerNode::Layer(Layer::new(8, "Active", doc.buffer.clone())),
            );
            // Isolate cache misses from the document's initial full-image COW.
            black_box(doc.composite.data_mut());
            let regions: Vec<_> = (0..21)
                .map(|i| Region::new(230 + i * 2, 240 + i, 64, 48))
                .collect();
            let start = Instant::now();
            let r = regions[0];
            doc.composite_rect(r.x, r.y, r.width, r.height);
            let cold = start.elapsed().as_secs_f64() * 1000.0;
            for r in &regions {
                doc.composite_rect(r.x, r.y, r.width, r.height);
            }
            let mut reference_times = Vec::new();
            let mut cached_times = Vec::new();
            for r in &regions {
                // Active edits do not change static cache identities.
                doc.buffer.set(r.x, r.y, Color::BLACK);
                let start = Instant::now();
                let mut expected = PixelBuffer::transparent(r.width, r.height);
                composite_region(&doc.layers, doc.active, &doc.buffer, *r, &mut expected);
                reference_times.push(start.elapsed().as_secs_f64() * 1000.0);
                let start = Instant::now();
                doc.composite_rect(r.x, r.y, r.width, r.height);
                cached_times.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(
                    doc.composite.region_rgba(r.x, r.y, r.width, r.height),
                    expected.data()
                );
                black_box(&doc.composite);
            }
            reference_times.sort_by(f64::total_cmp);
            cached_times.sort_by(f64::total_cmp);
            println!("{width}x{height}, grouped={grouped}, region64x48: cold={cold:.3}ms; uncached median={:.3}ms p95={:.3}ms; cached median={:.3}ms p95={:.3}ms",
                reference_times[10],reference_times[19],cached_times[10],cached_times[19]);
        }
    }
}

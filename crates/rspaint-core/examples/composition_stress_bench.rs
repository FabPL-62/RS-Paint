//! cargo run --release -p rspaint-core --example composition_stress_bench --offline
//! CPU-only stress scenes. Uniform static layers share storage to bound the
//! benchmark's RAM; every layer still participates in the blend calculations.
use rspaint_core::{
    layer::{composite_region, Region},
    Color, Document, Layer, LayerBlend, LayerGroup, LayerNode, PixelBuffer,
};
use std::{hint::black_box, time::Instant};

fn scene(width: u32, height: u32, kind: &str) -> Document {
    let mut doc = Document::new(width, height);
    let source = PixelBuffer::new(width, height, Color::rgba(60, 130, 190, 140));
    let modes = [
        LayerBlend::Normal,
        LayerBlend::Multiply,
        LayerBlend::Screen,
        LayerBlend::Overlay,
    ];
    let layer = |id: u64| {
        let mut layer = Layer::new(id, "Static", source.clone());
        layer.blend = modes[id as usize % 4];
        LayerNode::Layer(layer)
    };
    match kind {
        "active-top-32" | "opacity-changes-32" => {
            doc.layers.splice(0..0, (2..=32).map(layer));
            doc.layers.rotate_right(1);
        }
        "active-bottom-32" => {
            doc.layers.splice(0..0, (2..=32).map(layer));
        }
        "nested-8" => {
            let mut child = doc.layers.pop().unwrap();
            for depth in 0..8 {
                let mut group = LayerGroup::new(100 + depth, "Nested");
                group.opacity = 80 + depth as u8;
                group.blend = modes[depth as usize % 4];
                group.children = vec![layer(2 + depth * 2), child, layer(3 + depth * 2)];
                child = LayerNode::Group(group);
            }
            doc.layers = vec![child];
        }
        _ => unreachable!(),
    }
    black_box(doc.composite.data_mut());
    doc
}

fn main() {
    for (width, height) in [(1920, 1080), (6000, 4000)] {
        for kind in [
            "active-top-32",
            "active-bottom-32",
            "nested-8",
            "opacity-changes-32",
        ] {
            let mut doc = scene(width, height, kind);
            let regions: Vec<_> = (0..21)
                .map(|i| Region::new(230 + i * 2, 240 + i, 64, 48))
                .collect();
            for r in &regions {
                doc.composite_rect(r.x, r.y, r.width, r.height);
            }
            let mut reference = Vec::new();
            let mut cached = Vec::new();
            for (i, r) in regions.iter().enumerate() {
                doc.buffer.set(r.x, r.y, Color::BLACK);
                if kind == "opacity-changes-32" {
                    doc.layers[1].set_opacity(50 + i as u8);
                }
                let start = Instant::now();
                let mut expected = PixelBuffer::transparent(r.width, r.height);
                composite_region(&doc.layers, doc.active, &doc.buffer, *r, &mut expected);
                reference.push(start.elapsed().as_secs_f64() * 1000.0);
                let start = Instant::now();
                doc.composite_rect(r.x, r.y, r.width, r.height);
                cached.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(
                    doc.composite.region_rgba(r.x, r.y, r.width, r.height),
                    expected.data()
                );
            }
            reference.sort_by(f64::total_cmp);
            cached.sort_by(f64::total_cmp);
            println!("{width}x{height} {kind} region64x48: reference median={:.3}ms p95={:.3}ms; cache median={:.3}ms p95={:.3}ms",reference[10],reference[19],cached[10],cached[19]);
        }
    }
}

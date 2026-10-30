//! cargo run --release -p rspaint-core --example full_composite_cache_bench --offline
//! Full-size opacity updates: flat, sparse and detailed static content. All
//! outputs are checked against the uncached compositor after timing finishes.
use rspaint_core::{
    layer::{composite_region, Region},
    Color, Document, Layer, LayerGroup, LayerNode, PixelBuffer,
};
use std::time::Instant;

fn main() {
    let (width, height) = (6000, 4000);
    let grouped = std::env::args().any(|arg| arg == "--group");
    let active_below_detail = std::env::args().any(|arg| arg == "--active-below-detail");
    for kind in ["flat", "sparse", "detailed"] {
        let mut doc = Document::new(width, height);
        let mut source = PixelBuffer::new(
            width,
            height,
            if kind == "sparse" {
                Color::TRANSPARENT
            } else {
                Color::rgba(60, 130, 190, 140)
            },
        );
        if kind == "sparse" {
            for y in 1000..1080 {
                for p in source.row_mut(y, 1500, 200).chunks_exact_mut(4) {
                    p.copy_from_slice(&[60, 130, 190, 140]);
                }
            }
        } else if kind == "detailed" {
            let mut seed = 0x12345678u32;
            for p in source.data_mut().chunks_exact_mut(4) {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                p.copy_from_slice(&[seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 140]);
            }
        }
        // Sharing identical static source allocations limits fixture RAM, not
        // the number of per-layer blend calculations.
        doc.layers.splice(
            0..0,
            (2..8).map(|id| LayerNode::Layer(Layer::new(id, "Static", source.clone()))),
        );
        drop(source);
        doc.buffer = PixelBuffer::new(width, height, Color::rgba(150, 60, 20, 140));
        doc.active = 100;
        doc.next_id = 101;
        doc.layers.insert(
            0,
            LayerNode::Layer(Layer::new(100, "Active", doc.buffer.clone())),
        );
        if active_below_detail {
            let active = doc.layers.remove(0);
            doc.layers.insert(doc.layers.len() - 1, active);
        }
        if grouped {
            let mut group = LayerGroup::new(101, "Isolated group");
            group.opacity = 73;
            group.children = std::mem::take(&mut doc.layers);
            doc.layers.push(LayerNode::Group(group));
            doc.next_id = 102;
        }
        let start = Instant::now();
        doc.composite_all();
        let cold = start.elapsed().as_secs_f64() * 1000.0;
        println!(
            "  {kind}: cold direct scan workers: {}",
            doc.composition_cache_stats().direct_scan_workers
        );
        // Check the transition between the cached prefix and parallel suffix,
        // before a later full update could conceal an incorrect cold result.
        {
            let mut expected = PixelBuffer::transparent(width, height);
            composite_region(
                &doc.layers,
                doc.active,
                &doc.buffer,
                Region::new(0, 0, width, height),
                &mut expected,
            );
            assert_eq!(doc.composite, expected, "cold {kind}");
        }
        let mut samples = Vec::new();
        for opacity in [50, 60, 70] {
            rspaint_core::layer::find_node_mut(&mut doc.layers, 100)
                .unwrap()
                .set_opacity(opacity);
            let start = Instant::now();
            doc.composite_all();
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        let stats = doc.composition_cache_stats();
        println!("{width}x{height}, 8 layers, {kind}: cold={cold:.3}ms; opacity median={:.3}ms max={:.3}ms; stored={}B decoded-equivalent={}B entries={}",samples[1],samples[2],stats.stored_pixel_bytes,stats.uncompressed_pixel_bytes,stats.entries);
        assert!(stats.stored_pixel_bytes <= 32 * 1024 * 1024);
        println!("  direct scan workers: {}", stats.direct_scan_workers);
        if kind == "detailed" {
            assert!(stats.direct_scan_workers > 0);
        }
        let mut expected = PixelBuffer::transparent(width, height);
        let reference_start = Instant::now();
        composite_region(
            &doc.layers,
            doc.active,
            &doc.buffer,
            Region::new(0, 0, width, height),
            &mut expected,
        );
        assert_eq!(doc.composite, expected, "{kind}");
        let reference_ms = reference_start.elapsed().as_secs_f64() * 1000.0;
        println!("  {kind}: exact full-image reference comparison passed; uncached reference/check={reference_ms:.3}ms");
    }
}

//! Independent output bands for large, cache-hostile composition requests.
//! Scoped borrows keep source layers immutable and output slices disjoint.
use crate::{
    layer::{composite_region, Region},
    LayerNode, PixelBuffer,
};

pub(crate) fn timed_scan_candidate(region: Region) -> bool {
    region.width as u64 * region.height as u64 > 128 * 1024
}

/// Plain layers above the active branch depend on its changing backdrop.
/// Static group interiors and bottom stacks can instead benefit from cache;
/// do not discard their medium-size cached work for a direct scan.
pub(crate) fn has_uncached_upper_layers(nodes: &[LayerNode], active: u64) -> bool {
    let contains = |node: &LayerNode| {
        node.id() == active || crate::layer::find_node(node.children(), active).is_some()
    };
    let split = nodes.iter().rposition(contains).map_or(0, |i| i + 1);
    nodes[..split].iter().any(|node| match node {
        LayerNode::Layer(layer) => layer.id != active && layer.visible && layer.opacity > 0,
        LayerNode::Group(group) => {
            group.visible
                && group.opacity > 0
                && contains(node)
                && has_uncached_upper_layers(&group.children, active)
        }
    })
}

/// Use only work measured in this update, never a previous frame's timing.
/// Thresholds keep local edits and short/cheap scans on the cached path.
pub(crate) fn slow_scan_worthwhile(
    remaining: Region,
    sampled_pixels: u64,
    elapsed: std::time::Duration,
) -> bool {
    let remaining_pixels = remaining.width as u64 * remaining.height as u64;
    if remaining_pixels > 8 * 1024 * 1024 {
        return sampled_pixels >= 1024 * 1024
            && elapsed.as_secs_f64() * (1024 * 1024) as f64 / sampled_pixels as f64 >= 0.05;
    }
    // A short prefix also admits medium previews with substantial work left.
    // Cheap cached previews and small pointer rectangles stay serial.
    remaining_pixels >= 64 * 1024
        && sampled_pixels >= 16 * 1024
        && elapsed.as_secs_f64() >= 0.002
        && elapsed.as_secs_f64() * (1024 * 1024) as f64 / sampled_pixels as f64 >= 0.05
        && elapsed.as_secs_f64() * remaining_pixels as f64 / sampled_pixels as f64 >= 0.008
}

pub(crate) fn render(
    nodes: &[LayerNode],
    active: u64,
    pixels: &PixelBuffer,
    out: &mut PixelBuffer,
    region: Region,
    workers: usize,
) {
    if region.width == 0 || region.height == 0 {
        return;
    }
    let stride = out.width() as usize * 4;
    let rows_per_band = (region.height as usize).div_ceil(workers.max(1));
    let start = region.y as usize * stride;
    let end = (region.y + region.height) as usize * stride;
    let destination = &mut out.data_mut()[start..end];
    std::thread::scope(|scope| {
        for (index, band) in destination.chunks_mut(rows_per_band * stride).enumerate() {
            let base_y = region.y + index as u32 * rows_per_band as u32;
            scope.spawn(move || {
                let end_y = base_y + (band.len() / stride) as u32;
                for ty in (base_y..end_y).step_by(256) {
                    for tx in (region.x..region.x + region.width).step_by(256) {
                        let w = 256.min(region.x + region.width - tx);
                        let h = 256.min(end_y - ty);
                        let mut tile = PixelBuffer::transparent(w, h);
                        composite_region(
                            nodes,
                            active,
                            pixels,
                            Region::new(tx, ty, w, h),
                            &mut tile,
                        );
                        for row in 0..h {
                            let source = row as usize * w as usize * 4;
                            let dest = (ty + row - base_y) as usize * stride + tx as usize * 4;
                            band[dest..dest + w as usize * 4]
                                .copy_from_slice(&tile.data()[source..source + w as usize * 4]);
                        }
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Layer, LayerBlend, LayerGroup};

    #[test]
    fn medium_probe_preserves_cacheable_stacks_and_hidden_branches() {
        let pixels = PixelBuffer::transparent(1, 1);
        let active = LayerNode::Layer(Layer::new(1, "Active", pixels.clone()));
        let upper = LayerNode::Layer(Layer::new(2, "Upper", pixels));
        assert!(has_uncached_upper_layers(
            &[upper.clone(), active.clone()],
            1
        ));
        assert!(!has_uncached_upper_layers(
            &[active.clone(), upper.clone()],
            1
        ));
        let mut group = LayerGroup::new(3, "Static group");
        group.children.push(upper.clone());
        assert!(!has_uncached_upper_layers(
            &[LayerNode::Group(group.clone()), active.clone()],
            1
        ));
        group.children.push(active);
        let mut nodes = vec![LayerNode::Group(group)];
        assert!(has_uncached_upper_layers(&nodes, 1));
        nodes[0].set_visible(false);
        assert!(!has_uncached_upper_layers(&nodes, 1));
        nodes[0].set_visible(true);
        nodes[0].set_opacity(0);
        assert!(!has_uncached_upper_layers(&nodes, 1));
        nodes[0].set_opacity(73);
        nodes[0].children_mut().unwrap()[0].set_opacity(0);
        assert!(!has_uncached_upper_layers(&nodes, 1));
    }

    #[test]
    fn timed_admission_requires_a_large_slow_current_sample() {
        use std::time::Duration;
        let large = Region::new(7, 13, 6000, 3744);
        let sample = 6000 * 256;
        assert!(!timed_scan_candidate(Region::new(0, 0, 512, 256)));
        assert!(timed_scan_candidate(Region::new(0, 0, 512, 257)));
        assert!(slow_scan_worthwhile(
            large,
            1024 * 1024,
            Duration::from_millis(50)
        ));
        assert!(!slow_scan_worthwhile(
            large,
            1024 * 1024,
            Duration::from_millis(49)
        ));
        assert!(slow_scan_worthwhile(
            large,
            sample,
            Duration::from_millis(100)
        ));
        assert!(!slow_scan_worthwhile(
            large,
            sample,
            Duration::from_millis(20)
        ));
        assert!(!slow_scan_worthwhile(large, 100, Duration::from_secs(1)));
        assert!(!slow_scan_worthwhile(
            Region::new(0, 0, 64, 48),
            sample,
            Duration::from_secs(1)
        ));
        // Total elapsed time alone must not admit a cheap scan.
        assert!(!slow_scan_worthwhile(
            large,
            24_000_000,
            Duration::from_millis(50)
        ));
    }

    #[test]
    fn medium_admission_requires_slow_prefix_and_enough_remaining_work() {
        use std::time::Duration;
        let curve = Region::new(193, 257, 610, 340);
        let sample = 610 * 64;
        assert!(slow_scan_worthwhile(
            curve,
            sample,
            Duration::from_millis(6)
        ));
        assert!(!slow_scan_worthwhile(
            curve,
            sample,
            Duration::from_millis(1)
        ));
        assert!(!slow_scan_worthwhile(curve, 8000, Duration::from_millis(6)));
        assert!(!slow_scan_worthwhile(
            Region::new(7, 13, 64, 48),
            sample,
            Duration::from_secs(1)
        ));
        // The rate is high enough, but the projected remainder is below 8 ms.
        assert!(!slow_scan_worthwhile(
            Region::new(0, 0, 256, 256),
            sample,
            Duration::from_millis(3)
        ));
    }

    #[test]
    fn parallel_bands_match_reference_with_groups_dissolve_and_clipped_columns() {
        let mut pixels = PixelBuffer::transparent(263, 197);
        for (i, p) in pixels.data_mut().chunks_exact_mut(4).enumerate() {
            p.copy_from_slice(&[
                (i * 31) as u8,
                (i * 73) as u8,
                (i * 17) as u8,
                [0, 120, 255][i % 3],
            ]);
        }
        let source = PixelBuffer::new(263, 197, Color::rgba(30, 70, 140, 150));
        let region = Region::new(7, 5, 253, 189);
        for mode in LayerBlend::ALL {
            let mut group = LayerGroup::new(3, "Group");
            group.opacity = 61;
            group.blend = mode;
            let mut layer = Layer::new(2, "Top", source.clone());
            layer.blend = mode;
            group.children.push(LayerNode::Layer(layer));
            let nodes = vec![
                LayerNode::Group(group),
                LayerNode::Layer(Layer::new(1, "Active", pixels.clone())),
            ];
            let mut expected = PixelBuffer::new(263, 197, Color::rgb(130, 20, 90));
            let mut tile = PixelBuffer::transparent(region.width, region.height);
            composite_region(&nodes, 1, &pixels, region, &mut tile);
            expected.blit(&tile, region.x, region.y);
            for workers in [1, 4, 8] {
                let original = PixelBuffer::new(263, 197, Color::rgb(130, 20, 90));
                let mut actual = original.clone();
                render(&nodes, 1, &pixels, &mut actual, region, workers);
                assert_eq!(actual, expected, "{mode:?}, workers {workers}");
                assert_eq!(original.get(7, 5), Some(Color::rgb(130, 20, 90)));
            }
        }
    }
}

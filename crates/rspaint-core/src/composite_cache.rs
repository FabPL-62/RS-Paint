//! Bounded cache of static bottom stacks and isolated group interiors.
//! Never flattens an upper stack against transparency: blend modes depend on
//! the backdrop. Never retains the active layer's pixels in cache signatures.
use crate::{
    layer::{blend_region_at, composite_region, Region},
    Color, LayerBlend, LayerNode, PixelBuffer,
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Weak,
};

// Smaller than the composition scratch tiles to bound the first cache miss
// during a small pointer movement, especially across tile boundaries.
const TILE: u32 = 64;
const MAX_BYTES: usize = 32 * 1024 * 1024;
// Compression can fit a full large canvas without raising the pixel budget.
// Bound metadata separately by entry count (including very small edge tiles).
const MAX_ENTRIES: usize = 8192;

#[derive(Clone, Copy, Debug)]
struct Run {
    color: [u8; 4],
    count: u32,
}

#[derive(Debug)]
enum CachedPixels {
    Raw(PixelBuffer),
    Solid {
        width: u32,
        height: u32,
        color: Color,
    },
    Runs {
        width: u32,
        height: u32,
        runs: Vec<Run>,
    },
}

impl CachedPixels {
    fn encode(pixels: &PixelBuffer) -> Self {
        let raw = pixels.data();
        let first: [u8; 4] = raw[..4].try_into().unwrap();
        if raw.chunks_exact(4).all(|p| p == first) {
            return Self::Solid {
                width: pixels.width(),
                height: pixels.height(),
                color: Color::rgba(first[0], first[1], first[2], first[3]),
            };
        }
        // Avoid building throwaway runs for detailed/high-transition tiles.
        // This is only admission policy: raw storage is equally lossless.
        if raw.len() >= 32 * 4 {
            let mut previous = first;
            let mut changes = 0;
            let mut samples = 0;
            for p in raw.chunks_exact(4).take(64) {
                changes += usize::from(p != previous);
                previous = p.try_into().unwrap();
                samples += 1;
            }
            if changes > samples / 2 {
                return Self::Raw(pixels.clone());
            }
        }
        let max_runs = raw.len() / std::mem::size_of::<Run>();
        let mut runs: Vec<Run> = Vec::with_capacity(max_runs.min(64));
        for p in raw.chunks_exact(4) {
            if let Some(last) = runs.last_mut() {
                if p == last.color {
                    last.count += 1;
                    continue;
                }
            }
            // Stop early for detailed images; never store an encoding larger
            // than the raw pixels or spend a full pass building useless runs.
            if runs.len() + 1 >= max_runs {
                return Self::Raw(pixels.clone());
            }
            runs.push(Run {
                color: p.try_into().unwrap(),
                count: 1,
            });
        }
        if runs.capacity() * std::mem::size_of::<Run>() >= raw.len() {
            return Self::Raw(pixels.clone());
        }
        Self::Runs {
            width: pixels.width(),
            height: pixels.height(),
            runs,
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Self::Raw(pixels) => pixels.data().len(),
            Self::Solid { .. } => 4,
            Self::Runs { runs, .. } => runs.capacity() * std::mem::size_of::<Run>(),
        }
    }

    fn decode(&self) -> PixelBuffer {
        match self {
            Self::Raw(pixels) => pixels.clone(),
            Self::Solid {
                width,
                height,
                color,
            } => PixelBuffer::new(*width, *height, *color),
            Self::Runs {
                width,
                height,
                runs,
            } => {
                let mut pixels = PixelBuffer::transparent(*width, *height);
                let bytes = pixels.data_mut();
                let mut offset = 0;
                for run in runs {
                    let end = offset + run.count as usize * 4;
                    for p in bytes[offset..end].chunks_exact_mut(4) {
                        p.copy_from_slice(&run.color);
                    }
                    offset = end;
                }
                pixels
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Stamp {
    Node(u64, bool, u8, LayerBlend),
    Pixels(Weak<Vec<u8>>, u32, u32),
    End,
}

impl PartialEq for Stamp {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Node(a, b, c, d), Self::Node(e, f, g, h)) => (a, b, c, d) == (e, f, g, h),
            (Self::Pixels(a, w, h), Self::Pixels(b, x, y)) => {
                w == x && h == y && Weak::ptr_eq(a, b)
            }
            (Self::End, Self::End) => true,
            _ => false,
        }
    }
}

fn stamps(nodes: &[LayerNode], result: &mut Vec<Stamp>) {
    for node in nodes {
        result.push(Stamp::Node(
            node.id(),
            node.visible(),
            node.opacity(),
            node.blend(),
        ));
        match node {
            LayerNode::Layer(layer) => result.push(Stamp::Pixels(
                layer.pixels.weak_data(),
                layer.pixels.width(),
                layer.pixels.height(),
            )),
            LayerNode::Group(group) => stamps(&group.children, result),
        }
        result.push(Stamp::End);
    }
}

fn contains(nodes: &LayerNode, active: u64) -> bool {
    nodes.id() == active || nodes.children().iter().any(|node| contains(node, active))
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct Key(Option<u64>, u32, u32, u32, u32);

#[derive(Debug)]
struct Entry {
    stamps: Vec<Stamp>,
    pixels: CachedPixels,
    used: u64,
}

#[derive(Debug)]
struct ScopeState {
    signature: Vec<Stamp>,
    changed_update: Option<u64>,
}

#[derive(Debug, Default)]
pub(crate) struct CompositeCache {
    entries: HashMap<Key, Entry>,
    lru: BTreeSet<(u64, Key)>,
    bytes: usize,
    clock: u64,
    scopes: HashMap<Option<u64>, ScopeState>,
    update: u64,
    #[cfg(test)]
    hits: usize,
}

// Derived state is deliberately cold when a document is cloned.
impl Clone for CompositeCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl CompositeCache {
    /// Estimate streaming cost from at least one eighth of the pixel budget
    /// in current tiles of a root or visible group scope. Keep the sampled
    /// cache for small/local edits.
    pub(crate) fn direct_scan_worthwhile(
        &self,
        nodes: &[LayerNode],
        active: u64,
        region: Region,
    ) -> bool {
        self.direct_scan_with_budget(nodes, active, region, MAX_BYTES)
    }

    fn direct_scan_with_budget(
        &self,
        nodes: &[LayerNode],
        active: u64,
        region: Region,
        budget: usize,
    ) -> bool {
        self.direct_scope_with_budget(nodes, None, active, region, budget)
    }

    fn direct_scope_with_budget(
        &self,
        nodes: &[LayerNode],
        scope: Option<u64>,
        active: u64,
        region: Region,
        budget: usize,
    ) -> bool {
        let requested = region.width as usize * region.height as usize * 4;
        if requested <= budget {
            return false;
        }
        let split = nodes
            .iter()
            .rposition(|node| contains(node, active))
            .map_or(0, |i| i + 1);
        let mut signature = Vec::new();
        stamps(&nodes[split..], &mut signature);
        let mut valid_encoded = 0usize;
        let mut valid_decoded = 0usize;
        let mut scope_bytes = 0usize;
        for (key, entry) in &self.entries {
            if key.0 != scope {
                continue;
            }
            scope_bytes += entry.pixels.bytes();
            if split < nodes.len() && entry.stamps == signature {
                valid_encoded += entry.pixels.bytes();
                valid_decoded += key.3 as usize * key.4 as usize * 4;
            }
        }
        // Stale entries after a source/property change must not lock a newly
        // compressible stack into this path. Let ordinary admission relearn it.
        let worthwhile = valid_encoded >= scope_bytes * 9 / 10
            && valid_decoded >= (budget / 8).max(1)
            && requested as f64 * valid_encoded as f64 / valid_decoded as f64
                > budget as f64 * 1.25;
        worthwhile
            || nodes[..split].iter().any(|node| {
                if let LayerNode::Group(group) = node {
                    group.visible
                        && group.opacity > 0
                        && self.direct_scope_with_budget(
                            &group.children,
                            Some(group.id),
                            active,
                            region,
                            budget,
                        )
                } else {
                    false
                }
            })
    }
    pub(crate) fn stats(&self) -> (usize, usize, usize) {
        (
            self.bytes,
            self.entries
                .keys()
                .map(|key| key.3 as usize * key.4 as usize * 4)
                .sum(),
            self.entries.len(),
        )
    }
    pub(crate) fn begin_update(&mut self) {
        self.update = self.update.wrapping_add(1);
    }
    fn tile(
        &mut self,
        nodes: &[LayerNode],
        scope: Option<u64>,
        region: Region,
        active: u64,
        pixels: &PixelBuffer,
        signature: &[Stamp],
    ) -> PixelBuffer {
        let key = Key(scope, region.x, region.y, region.width, region.height);
        self.clock = self.clock.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(&key) {
            if entry.stamps.as_slice() == signature {
                self.lru.remove(&(entry.used, key));
                entry.used = self.clock;
                self.lru.insert((entry.used, key));
                #[cfg(test)]
                {
                    self.hits += 1;
                }
                return entry.pixels.decode();
            }
        }
        if let Some(old) = self.entries.remove(&key) {
            self.lru.remove(&(old.used, key));
            self.bytes -= old.pixels.bytes();
        }
        let mut out = PixelBuffer::transparent(region.width, region.height);
        composite_region(nodes, active, pixels, region, &mut out);
        let encoded = CachedPixels::encode(&out);
        let size = encoded.bytes();
        while self.bytes + size > MAX_BYTES || self.entries.len() >= MAX_ENTRIES {
            let Some((_, key)) = self.lru.pop_first() else {
                break;
            };
            self.bytes -= self.entries.remove(&key).unwrap().pixels.bytes();
        }
        self.bytes += size;
        self.lru.insert((self.clock, key));
        self.entries.insert(
            key,
            Entry {
                stamps: signature.to_vec(),
                pixels: encoded,
                used: self.clock,
            },
        );
        out
    }

    pub(crate) fn composite(
        &mut self,
        nodes: &[LayerNode],
        scope: Option<u64>,
        active: u64,
        pixels: &PixelBuffer,
        region: Region,
        out: &mut PixelBuffer,
    ) {
        // The suffix of the top-first tree is a bottom stack independent of
        // the active layer. Cache only that prefix of the compositing order.
        let split = nodes
            .iter()
            .rposition(|node| contains(node, active))
            .map_or(0, |i| i + 1);
        if split < nodes.len() {
            let mut signature = Vec::new();
            stamps(&nodes[split..], &mut signature);
            let signature_changed = self
                .scopes
                .get(&scope)
                .is_some_and(|old| old.signature != signature);
            if !self.scopes.contains_key(&scope) || signature_changed {
                if self.scopes.len() >= MAX_ENTRIES && !self.scopes.contains_key(&scope) {
                    self.scopes.clear();
                }
                self.scopes.insert(
                    scope,
                    ScopeState {
                        signature: signature.clone(),
                        changed_update: signature_changed.then_some(self.update),
                    },
                );
            }
            let changing = self
                .scopes
                .get(&scope)
                .is_some_and(|state| state.changed_update == Some(self.update));
            if changing {
                // A slider/animation can invalidate the static stack every
                // frame. Do not expand a small update into full cache tiles
                // while its signature keeps changing; resume after stability.
                composite_region(&nodes[split..], active, pixels, region, out);
            } else {
                let end_x = region.x + region.width;
                let end_y = region.y + region.height;
                for ty in ((region.y / TILE * TILE)..end_y).step_by(TILE as usize) {
                    for tx in ((region.x / TILE * TILE)..end_x).step_by(TILE as usize) {
                        let tile = self.tile(
                            &nodes[split..],
                            scope,
                            Region::new(
                                tx,
                                ty,
                                TILE.min(pixels.width() - tx),
                                TILE.min(pixels.height() - ty),
                            ),
                            active,
                            pixels,
                            &signature,
                        );
                        let x = tx.max(region.x);
                        let y = ty.max(region.y);
                        let w = (tx + tile.width()).min(end_x) - x;
                        let h = (ty + tile.height()).min(end_y) - y;
                        let stride = out.width() as usize * 4;
                        let data = out.data_mut();
                        for row in 0..h {
                            let source = ((y - ty + row) * tile.width() + x - tx) as usize * 4;
                            let dest = (y - region.y + row) as usize * stride
                                + (x - region.x) as usize * 4;
                            data[dest..dest + w as usize * 4]
                                .copy_from_slice(&tile.data()[source..source + w as usize * 4]);
                        }
                    }
                }
            }
        }
        for node in nodes[..split].iter().rev() {
            match node {
                LayerNode::Layer(layer) if layer.visible && layer.opacity > 0 => {
                    let src = if layer.id == active {
                        pixels
                    } else {
                        &layer.pixels
                    };
                    blend_region_at(
                        out,
                        src,
                        (region.x, region.y),
                        layer.opacity,
                        layer.blend,
                        (region.x, region.y),
                    );
                }
                LayerNode::Group(group) if group.visible && group.opacity > 0 => {
                    let mut inner = PixelBuffer::transparent(region.width, region.height);
                    self.composite(
                        &group.children,
                        Some(group.id),
                        active,
                        pixels,
                        region,
                        &mut inner,
                    );
                    blend_region_at(
                        out,
                        &inner,
                        (0, 0),
                        group.opacity,
                        group.blend,
                        (region.x, region.y),
                    );
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Layer, LayerGroup};

    #[test]
    fn nested_scan_admission_uses_only_visible_current_scopes() {
        let active = PixelBuffer::transparent(512, 256);
        let mut source = active.clone();
        for (i, p) in source.data_mut().chunks_exact_mut(4).enumerate() {
            p.copy_from_slice(&[i as u8, (i >> 8) as u8, 31, 255]);
        }
        let mut inner = LayerGroup::new(3, "Inner");
        inner.children = vec![
            LayerNode::Layer(Layer::new(2, "Active", active.clone())),
            LayerNode::Layer(Layer::new(1, "Static", source)),
        ];
        let mut outer = LayerGroup::new(4, "Outer");
        outer.children.push(LayerNode::Group(inner));
        let mut nodes = vec![LayerNode::Group(outer)];
        let mut cache = CompositeCache::default();
        let region = Region::new(0, 0, 512, 256);
        let mut tile = PixelBuffer::transparent(64, 64);
        cache.begin_update();
        cache.composite(
            &nodes,
            None,
            2,
            &active,
            Region::new(0, 0, 64, 64),
            &mut tile,
        );
        let budget = 128 * 1024;
        assert!(cache.direct_scan_with_budget(&nodes, 2, region, budget));
        assert!(!cache.direct_scan_with_budget(&nodes, 2, Region::new(0, 0, 64, 64), budget));
        nodes[0].set_visible(false);
        assert!(!cache.direct_scan_with_budget(&nodes, 2, region, budget));
        nodes[0].set_visible(true);
        nodes[0].set_opacity(0);
        assert!(!cache.direct_scan_with_budget(&nodes, 2, region, budget));
        nodes[0].set_opacity(73);
        assert!(cache.direct_scan_with_budget(&nodes, 2, region, budget));
        crate::layer::find_node_mut(&mut nodes, 1)
            .unwrap()
            .layer_mut()
            .unwrap()
            .pixels
            .fill(Color::WHITE);
        assert!(!cache.direct_scan_with_budget(&nodes, 2, region, budget));
        cache.begin_update();
        cache.composite(
            &nodes,
            None,
            2,
            &active,
            Region::new(0, 0, 64, 64),
            &mut tile,
        );
        assert!(!cache.direct_scan_with_budget(&nodes, 2, region, budget));
    }

    fn check(
        cache: &mut CompositeCache,
        nodes: &[LayerNode],
        active: u64,
        pixels: &PixelBuffer,
        x: u32,
    ) {
        cache.begin_update();
        let region = Region::new(x, 3, 23, 11);
        let mut expected = PixelBuffer::transparent(23, 11);
        composite_region(nodes, active, pixels, region, &mut expected);
        let mut actual = PixelBuffer::transparent(23, 11);
        cache.composite(nodes, None, active, pixels, region, &mut actual);
        assert_eq!(actual, expected, "active {active}, x {x}");
    }

    #[test]
    fn cached_composition_matches_reference_after_content_and_tree_changes() {
        for mode in LayerBlend::ALL {
            let mut pixels = PixelBuffer::new(300, 19, Color::rgba(160, 70, 30, 180));
            let static_pixels = PixelBuffer::new(300, 19, Color::rgba(20, 140, 220, 130));
            let mut group = LayerGroup::new(3, "Active group");
            group.blend = mode;
            group.opacity = 73;
            group.children = vec![
                LayerNode::Layer(Layer::new(2, "Active", pixels.clone())),
                LayerNode::Layer(Layer::new(4, "Static", static_pixels.clone())),
            ];
            let mut upper = LayerGroup::new(5, "Upper group");
            upper.blend = mode;
            upper.opacity = 51;
            upper
                .children
                .push(LayerNode::Layer(Layer::new(6, "Upper", static_pixels)));
            let mut nodes = vec![
                LayerNode::Group(upper),
                LayerNode::Group(group),
                LayerNode::Layer(Layer::new(
                    1,
                    "Base",
                    PixelBuffer::new(300, 19, Color::WHITE),
                )),
            ];
            let mut cache = CompositeCache::default();
            for x in [240, 250, 255, 256, 260] {
                check(&mut cache, &nodes, 2, &pixels, x);
            }
            assert!(cache.hits > 0);
            // Neither revisions nor editor invalidation are needed for direct mutations.
            pixels.set(256, 5, Color::BLACK);
            check(&mut cache, &nodes, 2, &pixels, 250);
            nodes[0].children_mut().unwrap()[0]
                .layer_mut()
                .unwrap()
                .pixels
                .set(256, 5, Color::BLACK);
            check(&mut cache, &nodes, 2, &pixels, 250);
            nodes[2].layer_mut().unwrap().pixels.data_mut()[256 * 4..257 * 4]
                .copy_from_slice(&[90, 10, 50, 255]);
            check(&mut cache, &nodes, 2, &pixels, 250);
            nodes[0].set_opacity(11);
            check(&mut cache, &nodes, 2, &pixels, 250);
            nodes[1].set_blend(LayerBlend::Dissolve);
            nodes[0].set_visible(false);
            check(&mut cache, &nodes, 2, &pixels, 250);
            nodes[0].set_visible(true);
            nodes.swap(0, 2);
            check(&mut cache, &nodes, 2, &pixels, 250);
            // Change drawing target, then edit an image that was previously cached.
            let new_pixels = nodes[0].layer().unwrap().pixels.clone();
            check(&mut cache, &nodes, 1, &new_pixels, 250);
            nodes[0].layer_mut().unwrap().pixels.fill(Color::BLACK);
            check(&mut cache, &nodes, 2, &pixels, 250);
            let saved = nodes.clone();
            nodes.remove(1);
            check(&mut cache, &nodes, 2, &pixels, 250);
            nodes = saved;
            check(&mut cache, &nodes, 2, &pixels, 250);
        }
    }

    #[test]
    fn cache_bounds_memory_and_does_not_retain_active_pixels() {
        let mut cache = CompositeCache::default();
        let mut pixels = PixelBuffer::new(33024, 256, Color::WHITE);
        for (i, pixel) in pixels.data_mut().chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[i as u8, (i >> 8) as u8, (i >> 16) as u8, 255]);
        }
        let nodes = vec![LayerNode::Layer(Layer::new(1, "Static", pixels.clone()))];
        let mut signature = Vec::new();
        stamps(&nodes, &mut signature);
        for x in (0..32768).step_by(256) {
            cache.tile(
                &nodes,
                None,
                Region::new(x, 0, 256, 256),
                2,
                &pixels,
                &signature,
            );
        }
        cache.tile(
            &nodes,
            None,
            Region::new(0, 0, 256, 256),
            2,
            &pixels,
            &signature,
        );
        cache.tile(
            &nodes,
            None,
            Region::new(32768, 0, 256, 256),
            2,
            &pixels,
            &signature,
        );
        assert!(cache.bytes <= MAX_BYTES);
        assert_eq!(cache.entries.len(), cache.lru.len());
        assert!(cache.lru.iter().all(|(used, key)| cache
            .entries
            .get(key)
            .is_some_and(|entry| entry.used == *used)));
        assert_eq!(cache.entries.len(), MAX_BYTES / (256 * 256 * 4));
        assert!(cache.entries.contains_key(&Key(None, 0, 0, 256, 256)));
        assert!(!cache.entries.contains_key(&Key(None, 256, 0, 256, 256)));
        assert!(cache.clone().entries.is_empty());
        let mut active = PixelBuffer::new(300, 19, Color::BLACK);
        let active_nodes = vec![LayerNode::Layer(Layer::new(2, "Active", active.clone()))];
        let weak = active.weak_data();
        let strong_count = weak.strong_count();
        let weak_count = weak.weak_count();
        check(&mut cache, &active_nodes, 2, &active, 250);
        assert_eq!(weak.strong_count(), strong_count);
        assert_eq!(weak.weak_count(), weak_count);
        // Weak signatures invalidate on unique-owner mutation without retaining
        // the source Vec or forcing a copy of its pixel allocation.
        let mut unique = PixelBuffer::new(3, 3, Color::WHITE);
        let identity = unique.weak_data();
        let address = unique.data().as_ptr();
        unique.set(1, 1, Color::BLACK);
        assert!(identity.upgrade().is_none());
        assert_eq!(address, unique.data().as_ptr());
        active.set(1, 1, Color::WHITE);
    }

    #[test]
    fn compressed_tiles_round_trip_rgba_and_fall_back_for_detailed_pixels() {
        for (width, height) in [(1, 1), (2, 1), (63, 65), (64, 64)] {
            for kind in 0..3 {
                let mut image = PixelBuffer::new(width, height, Color::rgba(30, 80, 140, 90));
                if kind == 1 {
                    for y in 0..height {
                        for x in 0..width {
                            image.set(
                                x,
                                y,
                                if x / 8 % 2 == 0 {
                                    Color::rgba(50, 60, 70, 0)
                                } else {
                                    Color::rgba(90, 100, 110, 127)
                                },
                            );
                        }
                    }
                } else if kind == 2 {
                    for (i, pixel) in image.data_mut().chunks_exact_mut(4).enumerate() {
                        pixel.copy_from_slice(&[
                            i as u8,
                            (i >> 8) as u8,
                            (i * 73) as u8,
                            (i * 37) as u8,
                        ]);
                    }
                }
                let encoded = CachedPixels::encode(&image);
                assert!(encoded.bytes() <= image.data().len());
                assert_eq!(encoded.decode(), image);
                if kind == 0 {
                    assert!(matches!(encoded, CachedPixels::Solid { .. }));
                }
                if kind == 1 && width == 64 {
                    assert!(matches!(encoded, CachedPixels::Runs { .. }));
                }
                if kind == 2 && width > 1 {
                    assert!(matches!(encoded, CachedPixels::Raw(_)));
                }
            }
        }
    }

    #[test]
    fn tiny_compressed_entries_still_obey_the_metadata_entry_limit() {
        let pixels = PixelBuffer::new(MAX_ENTRIES as u32 + 1, 1, Color::WHITE);
        let nodes = vec![LayerNode::Layer(Layer::new(1, "Static", pixels.clone()))];
        let mut signature = Vec::new();
        stamps(&nodes, &mut signature);
        let mut cache = CompositeCache::default();
        for x in 0..=MAX_ENTRIES as u32 {
            cache.tile(
                &nodes,
                None,
                Region::new(x, 0, 1, 1),
                2,
                &pixels,
                &signature,
            );
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert_eq!(cache.lru.len(), MAX_ENTRIES);
        assert_eq!(cache.bytes, MAX_ENTRIES * 4);
        assert!(!cache.entries.contains_key(&Key(None, 0, 0, 1, 1)));
    }

    #[test]
    fn changing_properties_bypass_cache_fill_then_resume_when_stable() {
        let pixels = PixelBuffer::new(300, 19, Color::rgba(180, 70, 20, 120));
        let mut nodes = vec![
            LayerNode::Layer(Layer::new(2, "Active", pixels.clone())),
            LayerNode::Layer(Layer::new(
                1,
                "Static",
                PixelBuffer::new(300, 19, Color::BLACK),
            )),
        ];
        let mut cache = CompositeCache::default();
        check(&mut cache, &nodes, 2, &pixels, 250);
        let clock = cache.clock;
        for opacity in 30..40 {
            nodes[1].set_opacity(opacity);
            check(&mut cache, &nodes, 2, &pixels, 250);
            assert_eq!(
                cache.clock, clock,
                "changing stack must not populate aligned tiles"
            );
        }
        check(&mut cache, &nodes, 2, &pixels, 250);
        assert!(cache.clock > clock);
        let hits = cache.hits;
        check(&mut cache, &nodes, 2, &pixels, 250);
        assert!(cache.hits > hits);
        nodes[1].set_opacity(45);
        cache.begin_update();
        let clock = cache.clock;
        for x in [0, 64, 128, 250] {
            let region = Region::new(x, 3, 23, 11);
            let mut out = PixelBuffer::transparent(23, 11);
            let mut expected = out.clone();
            composite_region(&nodes, 2, &pixels, region, &mut expected);
            cache.composite(&nodes, None, 2, &pixels, region, &mut out);
            assert_eq!(out, expected);
        }
        assert_eq!(
            cache.clock, clock,
            "every chunk in a changing update must bypass tile filling"
        );
    }

    #[test]
    fn direct_scan_admission_keeps_local_edits_cached_and_relearns_changed_content() {
        let mut source = PixelBuffer::new(512, 256, Color::WHITE);
        for (i, p) in source.data_mut().chunks_exact_mut(4).enumerate() {
            p.copy_from_slice(&[i as u8, (i >> 8) as u8, (i >> 16) as u8, 255]);
        }
        let active = PixelBuffer::transparent(512, 256);
        let mut nodes = vec![
            LayerNode::Layer(Layer::new(2, "Active", active.clone())),
            LayerNode::Layer(Layer::new(1, "Static", source)),
        ];
        let mut cache = CompositeCache::default();
        let mut signature = Vec::new();
        stamps(&nodes[1..], &mut signature);
        cache.tile(
            &nodes[1..],
            None,
            Region::new(0, 0, 64, 64),
            2,
            &active,
            &signature,
        );
        let budget = 64 * 64 * 4;
        // A cold prefix can trigger before filling the budget, but a tiny
        // sample must not decide the strategy for the rest of the image.
        assert!(cache.bytes < budget * 8);
        assert!(cache.direct_scan_with_budget(&nodes, 2, Region::new(0, 0, 512, 256), budget * 8));
        assert!(!cache.direct_scan_with_budget(
            &nodes,
            2,
            Region::new(0, 0, 512, 256),
            budget * 16
        ));
        assert!(cache.direct_scan_with_budget(&nodes, 2, Region::new(0, 0, 512, 256), budget));
        assert!(!cache.direct_scan_with_budget(&nodes, 2, Region::new(0, 0, 64, 64), budget));
        nodes[1].layer_mut().unwrap().pixels.fill(Color::WHITE);
        assert!(!cache.direct_scan_with_budget(&nodes, 2, Region::new(0, 0, 512, 256), budget));
        signature.clear();
        stamps(&nodes[1..], &mut signature);
        cache.tile(
            &nodes[1..],
            None,
            Region::new(0, 0, 64, 64),
            2,
            &active,
            &signature,
        );
        assert_eq!(cache.bytes, 4);
        assert!(!cache.direct_scan_with_budget(&nodes, 2, Region::new(0, 0, 512, 256), budget));
    }
}

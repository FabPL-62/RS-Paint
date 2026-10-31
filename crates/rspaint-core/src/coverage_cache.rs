//! Bounded alpha coverage metadata, never retaining source pixel allocations.
use crate::{buffer::HISTORY_BLOCK_SIZE, layer::Region, LayerNode, PixelBuffer};
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

const MAX_ENTRIES: usize = 64;
const MAX_BLOCK_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
struct Entry {
    source: Weak<Vec<u8>>,
    size: [usize; 2],
    bounds: Option<Region>,
    versions: Option<Arc<Vec<u64>>>,
    blocks: Vec<Option<Region>>,
}

impl Entry {
    fn bytes(&self) -> usize {
        self.blocks.capacity() * std::mem::size_of::<Option<Region>>()
            + self
                .versions
                .as_ref()
                .map_or(0, |v| v.capacity() * std::mem::size_of::<u64>())
    }
}

#[derive(Debug, Default)]
pub(crate) struct CoverageCache {
    entries: VecDeque<Entry>,
    block_bytes: usize,
    #[cfg(test)]
    scanned_blocks: usize,
}

impl Clone for CoverageCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl CoverageCache {
    fn pixels(&mut self, pixels: &PixelBuffer) -> Option<Region> {
        self.pixels_with_blocks(pixels, false)
    }

    fn pixels_with_blocks(&mut self, pixels: &PixelBuffer, detailed: bool) -> Option<Region> {
        let source = pixels.weak_data();
        let size = pixels.size();
        if let Some(index) = self
            .entries
            .iter()
            .position(|e| e.size == size && Weak::ptr_eq(&source, &e.source))
        {
            let entry = self.entries.remove(index).unwrap();
            if !detailed || entry.bounds.is_none() || !entry.blocks.is_empty() {
                let bounds = entry.bounds;
                self.entries.push_front(entry);
                return bounds;
            }
            self.block_bytes -= entry.bytes();
        }
        // Dead pixel identities can still provide valid bounds for unchanged
        // blocks. They retain versions and metadata, never the source pixels.
        self.entries
            .retain(|e| e.source.strong_count() > 0 || !e.blocks.is_empty());
        let mut entry = Entry {
            source,
            size,
            bounds: None,
            versions: None,
            blocks: Vec::new(),
        };
        if pixels.known_empty_alpha() {
            entry.bounds = None;
        } else if !detailed && full_edges(pixels) {
            entry.bounds = Some(Region::new(0, 0, pixels.width(), pixels.height()));
        } else {
            let versions = pixels.history_versions();
            let bytes = versions.capacity() * std::mem::size_of::<u64>()
                + versions.len() * std::mem::size_of::<Option<Region>>();
            if bytes <= MAX_BLOCK_BYTES {
                let previous = self
                    .entries
                    .iter()
                    .filter(|e| {
                        e.size == size && e.blocks.len() == versions.len() && e.versions.is_some()
                    })
                    .map(|e| {
                        (
                            e,
                            e.versions
                                .as_ref()
                                .unwrap()
                                .iter()
                                .zip(versions.iter())
                                .filter(|(a, b)| a == b)
                                .count(),
                        )
                    })
                    .max_by_key(|(_, matching)| *matching)
                    .filter(|(_, matching)| *matching > 0)
                    .map(|(e, _)| e);
                let cols = pixels.width().div_ceil(HISTORY_BLOCK_SIZE) as usize;
                let mut blocks = Vec::with_capacity(versions.len());
                #[cfg(test)]
                let mut scanned = 0;
                for (index, version) in versions.iter().enumerate() {
                    let x = (index % cols) as u32 * HISTORY_BLOCK_SIZE;
                    let y = (index / cols) as u32 * HISTORY_BLOCK_SIZE;
                    let region = Region::new(
                        x,
                        y,
                        HISTORY_BLOCK_SIZE.min(pixels.width() - x),
                        HISTORY_BLOCK_SIZE.min(pixels.height() - y),
                    );
                    if let Some(alpha) = pixels.known_block_alpha(*version) {
                        blocks.push((alpha != 0).then_some(region));
                        continue;
                    }
                    if let Some(old) =
                        previous.filter(|e| e.versions.as_ref().unwrap()[index] == *version)
                    {
                        blocks.push(old.blocks[index]);
                    } else {
                        blocks.push(scan_region(pixels, region));
                        #[cfg(test)]
                        {
                            scanned += 1;
                        }
                    }
                }
                #[cfg(test)]
                {
                    self.scanned_blocks += scanned;
                }
                entry.bounds = blocks.iter().filter_map(|b| *b).reduce(union);
                entry.blocks = blocks;
                entry.versions = Some(versions);
            } else {
                // Large metadata must not expand this derived cache unboundedly.
                entry.bounds =
                    scan_region(pixels, Region::new(0, 0, pixels.width(), pixels.height()));
            }
        }
        let bounds = entry.bounds;
        self.store(entry);
        bounds
    }

    fn store(&mut self, mut entry: Entry) {
        if entry.bytes() > MAX_BLOCK_BYTES {
            entry.blocks = Vec::new();
            entry.versions = None;
        }
        let bytes = entry.bytes();
        while self.entries.len() >= MAX_ENTRIES || self.block_bytes + bytes > MAX_BLOCK_BYTES {
            self.block_bytes -= self.entries.pop_back().unwrap().bytes();
        }
        self.block_bytes += bytes;
        self.entries.push_front(entry);
    }

    /// Ignore this node's own visibility/opacity: showing it or raising its
    /// opacity can expose its entire content. Honor those of its descendants.
    pub(crate) fn node(
        &mut self,
        node: &LayerNode,
        active: u64,
        working: &PixelBuffer,
    ) -> Option<Region> {
        match node {
            LayerNode::Layer(layer) => self.pixels(if layer.id == active {
                working
            } else {
                &layer.pixels
            }),
            LayerNode::Group(group) => group
                .children
                .iter()
                .filter(|child| child.visible() && child.opacity() > 0)
                .filter_map(|child| self.node(child, active, working))
                .reduce(union),
        }
    }

    pub(crate) fn regions(
        &mut self,
        node: &LayerNode,
        active: u64,
        working: &PixelBuffer,
        limit: usize,
    ) -> Option<Vec<Region>> {
        let mut regions = Vec::new();
        self.collect_regions(node, active, working, limit, &mut regions)
            .then_some(regions)
    }

    fn collect_regions(
        &mut self,
        node: &LayerNode,
        active: u64,
        working: &PixelBuffer,
        limit: usize,
        regions: &mut Vec<Region>,
    ) -> bool {
        match node {
            LayerNode::Group(group) => group
                .children
                .iter()
                .filter(|n| n.visible() && n.opacity() > 0)
                .all(|child| self.collect_regions(child, active, working, limit, regions)),
            LayerNode::Layer(layer) => {
                let pixels = if layer.id == active {
                    working
                } else {
                    &layer.pixels
                };
                let Some(bounds) = self.pixels(pixels) else {
                    return true;
                };
                let source = pixels.weak_data();
                let has_blocks = self.entries.iter().any(|e| {
                    e.size == pixels.size()
                        && Weak::ptr_eq(&source, &e.source)
                        && !e.blocks.is_empty()
                });
                if !has_blocks && !pixels.mostly_known_transparent() {
                    return add_region(regions, bounds, limit);
                }
                self.pixels_with_blocks(pixels, true);
                let entry = self
                    .entries
                    .iter()
                    .find(|e| e.size == pixels.size() && Weak::ptr_eq(&source, &e.source))
                    .unwrap();
                if entry.blocks.is_empty() {
                    return add_region(regions, bounds, limit);
                }
                entry
                    .blocks
                    .iter()
                    .filter_map(|b| *b)
                    .all(|r| add_region(regions, r, limit))
            }
        }
    }
}

fn add_region(regions: &mut Vec<Region>, mut region: Region, limit: usize) -> bool {
    let mut i = 0;
    while i < regions.len() {
        let other = regions[i];
        if region.x <= other.x + other.width
            && other.x <= region.x + region.width
            && region.y <= other.y + other.height
            && other.y <= region.y + region.height
        {
            region = union(region, regions.swap_remove(i));
            i = 0;
        } else {
            i += 1;
        }
    }
    regions.push(region);
    regions.len() <= limit
}

fn union(a: Region, b: Region) -> Region {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Region::new(
        x,
        y,
        (a.x + a.width).max(b.x + b.width) - x,
        (a.y + a.height).max(b.y + b.height) - y,
    )
}

fn full_edges(pixels: &PixelBuffer) -> bool {
    let (w, h) = (pixels.width(), pixels.height());
    let bytes = pixels.data();
    let stride = w as usize * 4;
    // Dense images usually establish all four bounds with just four pixels.
    // A visible pixel on each edge proves full coverage bounds exactly.
    bytes[..stride].chunks_exact(4).any(|p| p[3] != 0)
        && bytes[bytes.len() - stride..]
            .chunks_exact(4)
            .any(|p| p[3] != 0)
        && (0..h as usize).any(|y| bytes[y * stride + 3] != 0)
        && (0..h as usize).any(|y| bytes[(y + 1) * stride - 1] != 0)
}

#[cfg(test)]
fn scan(pixels: &PixelBuffer) -> Option<Region> {
    scan_region(pixels, Region::new(0, 0, pixels.width(), pixels.height()))
}

fn scan_region(pixels: &PixelBuffer, region: Region) -> Option<Region> {
    let (mut left, mut top, mut right, mut bottom) = (
        region.x + region.width,
        region.y + region.height,
        region.x,
        region.y,
    );
    for y in region.y..region.y + region.height {
        let start = (y as usize * pixels.width() as usize + region.x as usize) * 4;
        let row = &pixels.data()[start..start + region.width as usize * 4];
        for (x, p) in row.chunks_exact(4).enumerate() {
            if p[3] != 0 {
                let x = region.x + x as u32;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    (right > left && bottom > top).then(|| Region::new(left, top, right - left, bottom - top))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Layer, LayerGroup};
    fn bounds(region: Option<Region>) -> Option<(u32, u32, u32, u32)> {
        region.map(|r| (r.x, r.y, r.width, r.height))
    }

    #[test]
    fn incremental_bounds_use_uniform_seeds_and_raw_writes_remain_conservative() {
        let mut pixels = PixelBuffer::transparent(259, 259);
        let mut cache = CoverageCache::default();
        assert_eq!(bounds(cache.pixels(&pixels)), None);
        assert_eq!(cache.scanned_blocks, 0);
        pixels.set(128, 128, Color::BLACK);
        assert_eq!(bounds(cache.pixels(&pixels)), Some((128, 128, 1, 1)));
        assert_eq!(cache.scanned_blocks, 1);
        pixels
            .row_mut(127, 127, 3)
            .copy_from_slice(&[1, 2, 3, 1, 4, 5, 6, 1, 7, 8, 9, 1]);
        assert_eq!(bounds(cache.pixels(&pixels)), Some((127, 127, 3, 2)));
        assert_eq!(cache.scanned_blocks, 3);
        pixels.fill(Color::TRANSPARENT);
        assert_eq!(bounds(cache.pixels(&pixels)), None);
        assert_eq!(cache.scanned_blocks, 3);
        let last_alpha = pixels.data().len() - 1;
        pixels.data_mut()[last_alpha] = 1;
        assert_eq!(bounds(cache.pixels(&pixels)), Some((258, 258, 1, 1)));
        assert_eq!(cache.scanned_blocks, 12);
        assert_eq!(
            cache.block_bytes,
            cache.entries.iter().map(Entry::bytes).sum::<usize>()
        );
        assert!(cache.block_bytes <= MAX_BLOCK_BYTES);
    }

    #[test]
    fn first_coverage_query_reads_only_blocks_modified_since_initialization() {
        let mut cache = CoverageCache::default();
        let empty = PixelBuffer::new(259, 259, Color::rgba(30, 60, 90, 0));
        assert_eq!(bounds(cache.pixels(&empty)), None);
        assert_eq!(cache.scanned_blocks, 0);
        let mut fresh = PixelBuffer::transparent(259, 259);
        fresh.set(128, 128, Color::BLACK);
        assert_eq!(bounds(cache.pixels(&fresh)), Some((128, 128, 1, 1)));
        assert_eq!(cache.scanned_blocks, 1);
        let cropped = fresh.cropped(120, 120, 139, 139);
        assert_eq!(bounds(cache.pixels(&cropped)), Some((8, 8, 1, 1)));
        let resized = fresh.resized(390, 390, Color::TRANSPARENT);
        assert_eq!(bounds(cache.pixels(&resized)), Some((128, 128, 1, 1)));
        let mut clone = fresh.clone();
        clone.clear();
        assert_eq!(bounds(cache.pixels(&clone)), None);
        assert_eq!(bounds(cache.pixels(&fresh)), Some((128, 128, 1, 1)));
    }

    #[test]
    fn version_reuse_preserves_forked_edits_and_restored_history() {
        let mut document = crate::Document::new(259, 259);
        document.buffer.fill(Color::TRANSPARENT);
        document.buffer.set(1, 1, Color::BLACK);
        let snapshot = document.history_snapshot(None);
        let mut cache = CoverageCache::default();
        cache.pixels(&document.buffer);
        let mut left = document.buffer.clone();
        let mut right = document.buffer.clone();
        left.set(128, 128, Color::BLACK);
        right.set(258, 258, Color::BLACK);
        assert_eq!(bounds(cache.pixels(&left)), Some((1, 1, 128, 128)));
        assert_eq!(bounds(cache.pixels(&right)), Some((1, 1, 258, 258)));
        document.buffer = right;
        document.restore_history(snapshot);
        assert_eq!(bounds(cache.pixels(&document.buffer)), Some((1, 1, 1, 1)));
        assert!(cache.block_bytes <= MAX_BLOCK_BYTES);
    }

    #[test]
    fn retained_block_metadata_evicts_within_budget_without_retaining_pixels() {
        let mut cache = CoverageCache::default();
        let pixels = PixelBuffer::transparent(259, 259);
        let weak = pixels.weak_data();
        cache.pixels(&pixels);
        drop(pixels);
        assert_eq!(weak.strong_count(), 0);
        // Exercise derived-metadata admission without allocating 90 huge RGBA
        // images. Each entry describes the grid of an 8192x8192 empty image.
        for generation in 0..90 {
            cache.store(Entry {
                source: Weak::new(),
                size: [8192, 8192],
                bounds: None,
                versions: Some(Arc::new(vec![generation; 4096])),
                blocks: vec![None; 4096],
            });
            assert!(cache.entries.len() <= MAX_ENTRIES);
            assert!(cache.block_bytes <= MAX_BLOCK_BYTES);
        }
        assert!(
            cache.entries.len() < MAX_ENTRIES,
            "the byte budget must evict before the entry limit"
        );
        assert_eq!(
            cache.block_bytes,
            cache.entries.iter().map(Entry::bytes).sum::<usize>()
        );
        cache.store(Entry {
            source: Weak::new(),
            size: [1, 1],
            bounds: None,
            versions: None,
            blocks: Vec::with_capacity(MAX_BLOCK_BYTES / std::mem::size_of::<Option<Region>>() + 1),
        });
        assert!(cache.entries.front().unwrap().blocks.is_empty());
        assert!(cache.block_bytes <= MAX_BLOCK_BYTES);
    }

    #[test]
    fn weak_coverage_tracks_writes_shrinking_content_and_forked_buffers() {
        let mut pixels = PixelBuffer::new(17, 13, Color::rgba(30, 60, 90, 0));
        let mut cache = CoverageCache::default();
        assert_eq!(bounds(cache.pixels(&pixels)), None);
        pixels.set(7, 8, Color::BLACK);
        assert_eq!(bounds(cache.pixels(&pixels)), Some((7, 8, 1, 1)));
        assert_eq!(bounds(cache.pixels(&pixels)), Some((7, 8, 1, 1)));
        let original = pixels.clone();
        pixels.set(0, 0, Color::BLACK);
        assert_eq!(bounds(cache.pixels(&pixels)), Some((0, 0, 8, 9)));
        assert_eq!(bounds(cache.pixels(&original)), Some((7, 8, 1, 1)));
        pixels.set(0, 0, Color::TRANSPARENT);
        assert_eq!(bounds(cache.pixels(&pixels)), Some((7, 8, 1, 1)));
        pixels.fill(Color::TRANSPARENT);
        assert_eq!(bounds(cache.pixels(&pixels)), None);
        pixels.row_mut(12, 16, 1).copy_from_slice(&[10, 20, 30, 1]);
        assert_eq!(bounds(cache.pixels(&pixels)), Some((16, 12, 1, 1)));
        pixels.data_mut().fill(0);
        assert_eq!(bounds(cache.pixels(&pixels)), None);
        let handle = pixels.shared_data();
        let owners = std::sync::Arc::strong_count(&handle);
        cache.pixels(&pixels);
        assert_eq!(std::sync::Arc::strong_count(&handle), owners);
    }

    #[test]
    fn edge_proof_and_metadata_limit_are_exact_and_bounded() {
        let mut pixels = PixelBuffer::transparent(9, 7);
        for (x, y) in [(0, 3), (8, 3), (4, 0), (4, 6)] {
            pixels.set(x, y, Color::BLACK);
        }
        assert_eq!(bounds(scan(&pixels)), Some((0, 0, 9, 7)));
        assert_eq!(
            bounds(scan(&PixelBuffer::new(1, 1, Color::WHITE))),
            Some((0, 0, 1, 1))
        );
        let mut cache = CoverageCache::default();
        let buffers: Vec<_> = (1..=80)
            .map(|w| PixelBuffer::new(w, 1, Color::WHITE))
            .collect();
        for buffer in &buffers {
            cache.pixels(buffer);
            assert!(cache.entries.len() <= MAX_ENTRIES);
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert!(cache.clone().entries.is_empty());
    }

    #[test]
    fn groups_ignore_own_flags_but_honor_children_and_active_working_pixels() {
        let mut working = PixelBuffer::transparent(30, 30);
        working.set(5, 6, Color::rgba(20, 30, 40, 1));
        let active = Layer::new(1, "Active placeholder", PixelBuffer::transparent(1, 1));
        let mut far = PixelBuffer::transparent(30, 30);
        far.set(20, 20, Color::BLACK);
        let mut other = Layer::new(2, "Hidden", far);
        other.visible = false;
        let mut group = LayerGroup::new(3, "Hidden target");
        group.visible = false;
        group.opacity = 0;
        group.children = vec![LayerNode::Layer(active), LayerNode::Layer(other)];
        let mut node = LayerNode::Group(group);
        let mut cache = CoverageCache::default();
        assert_eq!(bounds(cache.node(&node, 1, &working)), Some((5, 6, 1, 1)));
        node.children_mut().unwrap()[1].set_visible(true);
        assert_eq!(bounds(cache.node(&node, 1, &working)), Some((5, 6, 16, 15)));
        node.children_mut().unwrap()[0].set_opacity(0);
        assert_eq!(bounds(cache.node(&node, 1, &working)), Some((20, 20, 1, 1)));
        node.children_mut().unwrap()[1].set_opacity(0);
        assert_eq!(bounds(cache.node(&node, 1, &working)), None);
    }
}

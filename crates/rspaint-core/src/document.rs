//! The document being edited.

use crate::buffer::PixelBuffer;
use crate::color::Color;
use crate::layer::{
    composite_region, find_node, find_node_mut, map_layer_pixels, Layer, LayerNode, LayerStack,
    Region,
};

/// Default canvas width used by JS Paint.
pub const DEFAULT_CANVAS_WIDTH: u32 = 683;
/// Default canvas height used by JS Paint.
pub const DEFAULT_CANVAS_HEIGHT: u32 = 384;

/// Pixel payload retained by the derived composition cache. Excludes hash-map,
/// signatures and allocator metadata; `entries` separately bounds that growth.
#[derive(Clone, Copy, Debug)]
pub struct CompositionCacheStats {
    pub stored_pixel_bytes: usize,
    pub uncompressed_pixel_bytes: usize,
    pub entries: usize,
    /// Workers used for the last direct scan; zero used the cached compositor.
    pub direct_scan_workers: usize,
}

/// The state of a single image being edited.
///
/// The document is a stack of layers. `buffer` always holds the *active*
/// layer's working pixels (every editor operation reads and writes it), while
/// `composite` holds the flattened image shown on screen and saved to disk.
/// During an interactive edit the active tree entry is only a placeholder;
/// read its pixels through `buffer` or call `sync_active` before copying the tree.
#[derive(Clone, Debug)]
pub struct Document {
    cache: crate::composite_cache::CompositeCache,
    coverage_cache: crate::coverage_cache::CoverageCache,
    editing_active: Option<u64>,
    direct_scan_workers: usize,
    /// The working pixels of the active layer.
    pub buffer: PixelBuffer,
    /// The flattened image: all visible layers composited.
    pub composite: PixelBuffer,
    /// The layer tree, topmost first.
    pub layers: Vec<LayerNode>,
    /// The id of the active node (a layer or a group).
    pub active: u64,
    /// The next unused layer id.
    pub next_id: u64,
    /// Counter for naming new layers.
    pub layer_counter: u32,
    /// Counter for naming new groups.
    pub group_counter: u32,
    /// Changes whenever the active layer's working pixels do.
    pub active_revision: u64,
    /// Whether the image supports transparency (JS Paint's "Transparent" mode).
    pub transparent: bool,
    /// The color used when clearing or starting a new image.
    pub background: Color,
    /// The display name, e.g. `"Untitled"`.
    pub name: String,
    /// Whether the document has unsaved changes.
    pub modified: bool,
}

impl Document {
    /// At most 32 separated content regions; falls back to a single bounding
    /// rectangle when fragmentation or dense coverage would make batching costly.
    pub fn layer_content_regions(&mut self, id: u64) -> Vec<Region> {
        let Some(bounds) = self.layer_content_bounds(id) else {
            return Vec::new();
        };
        let node = find_node(&self.layers, id).unwrap();
        let Some(regions) = self
            .coverage_cache
            .regions(node, self.active, &self.buffer, 32)
        else {
            return vec![bounds];
        };
        let regions: Vec<_> = regions
            .into_iter()
            .filter_map(|r| {
                let w = r.width.min(self.width().saturating_sub(r.x));
                let h = r.height.min(self.height().saturating_sub(r.y));
                (w > 0 && h > 0).then(|| Region::new(r.x, r.y, w, h))
            })
            .collect();
        let area = regions.iter().fold(0u64, |sum, r| {
            sum.saturating_add(r.width as u64 * r.height as u64)
        });
        if regions.len() > 1 && area.saturating_mul(4) < bounds.width as u64 * bounds.height as u64
        {
            regions
        } else {
            vec![bounds]
        }
    }

    /// Conservative bounds of nonzero alpha, clipped to the canvas. The target
    /// node's own visibility and opacity are ignored; descendants honor theirs.
    /// Active-layer pixels always come from the canonical working buffer.
    pub fn layer_content_bounds(&mut self, id: u64) -> Option<Region> {
        let node = find_node(&self.layers, id)?;
        let region = self.coverage_cache.node(node, self.active, &self.buffer)?;
        let w = region.width.min(self.width().saturating_sub(region.x));
        let h = region.height.min(self.height().saturating_sub(region.y));
        (w > 0 && h > 0).then(|| Region::new(region.x, region.y, w, h))
    }

    pub fn composition_cache_stats(&self) -> CompositionCacheStats {
        let (stored_pixel_bytes, uncompressed_pixel_bytes, entries) = self.cache.stats();
        CompositionCacheStats {
            stored_pixel_bytes,
            uncompressed_pixel_bytes,
            entries,
            direct_scan_workers: self.direct_scan_workers,
        }
    }
    /// Creates a new opaque white document of the given size.
    pub fn new(width: u32, height: u32) -> Self {
        let buffer = PixelBuffer::new(width, height, Color::WHITE);
        let layers = vec![LayerNode::Layer(Layer::new(
            1,
            "Background",
            buffer.clone(),
        ))];
        Self {
            cache: Default::default(),
            coverage_cache: Default::default(),
            editing_active: None,
            direct_scan_workers: 0,
            composite: buffer.clone(),
            buffer,
            layers,
            active: 1,
            next_id: 2,
            layer_counter: 1,
            group_counter: 1,
            active_revision: 0,
            transparent: false,
            background: Color::WHITE,
            name: String::from("Untitled"),
            modified: false,
        }
    }

    /// The document width in pixels.
    pub fn width(&self) -> u32 {
        self.buffer.width()
    }

    /// The document height in pixels.
    pub fn height(&self) -> u32 {
        self.buffer.height()
    }

    /// Replaces the whole image with `pixels` on a single `Background` layer.
    pub fn replace_pixels(&mut self, pixels: PixelBuffer) {
        self.direct_scan_workers = 0;
        self.editing_active = None;
        self.cache = Default::default();
        self.coverage_cache = Default::default();
        let layer_id = self.next_id;
        self.next_id += 1;
        self.layers = vec![LayerNode::Layer(Layer::new(
            layer_id,
            "Background",
            pixels.clone(),
        ))];
        self.active = layer_id;
        self.buffer = pixels.clone();
        self.composite = pixels;
        self.layer_counter = 1;
        self.group_counter = 1;
        self.active_revision = 0;
    }

    /// Writes the working buffer back into the active layer's tree entry.
    pub fn sync_active(&mut self) {
        if let Some(LayerNode::Layer(layer)) = find_node_mut(&mut self.layers, self.active) {
            layer.pixels = self.buffer.clone();
            layer.revision = self.active_revision;
        }
        self.editing_active = None;
    }

    /// Releases the stale active-layer mirror during an interactive edit.
    /// The working buffer remains authoritative; active-layer composition uses
    /// it directly. sync_active restores the tree entry before snapshots, layer
    /// switches and tree operations. External owners still get normal COW.
    pub fn prepare_active_edit(&mut self) {
        if self.editing_active == Some(self.active) {
            return;
        }
        if let Some(LayerNode::Layer(layer)) = find_node_mut(&mut self.layers, self.active) {
            layer.pixels = PixelBuffer::transparent(1, 1);
            self.editing_active = Some(self.active);
        }
    }

    /// Loads the active layer's pixels into the working buffer.
    pub fn refresh_buffer(&mut self) {
        if self.editing_active == Some(self.active) {
            return;
        }
        match find_node(&self.layers, self.active) {
            Some(LayerNode::Layer(layer)) => self.buffer = layer.pixels.clone(),
            _ => {
                // A group is active: there is no single layer to edit, but the
                // buffer still has to keep the document dimensions.
                let (width, height) = self
                    .layers
                    .iter()
                    .find_map(LayerNode::pixel_size)
                    .unwrap_or((self.buffer.width(), self.buffer.height()));
                if self.buffer.width() != width || self.buffer.height() != height {
                    self.buffer = PixelBuffer::transparent(width, height);
                }
            }
        }
    }

    /// Makes `id` the active node, after saving the previous layer's pixels.
    pub fn activate(&mut self, id: u64) {
        if id == self.active || find_node(&self.layers, id).is_none() {
            return;
        }
        self.sync_active();
        self.active = id;
        self.refresh_buffer();
    }

    /// Recomputes the flattened image.
    pub fn composite_all(&mut self) {
        let (width, height) = (self.buffer.width(), self.buffer.height());
        if self.composite.width() != width || self.composite.height() != height {
            self.composite = PixelBuffer::transparent(width, height);
        }
        self.composite_rect(0, 0, width, height);
    }

    /// Recomputes a region of the flattened image.
    pub fn composite_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if self.composite.width() != self.buffer.width()
            || self.composite.height() != self.buffer.height()
        {
            self.composite = PixelBuffer::transparent(self.buffer.width(), self.buffer.height());
        }
        let end_x = x.saturating_add(width).min(self.width());
        let end_y = y.saturating_add(height).min(self.height());
        if x >= end_x || y >= end_y {
            return;
        }
        let region = Region::new(x, y, end_x - x, end_y - y);
        self.direct_scan_workers = 0;
        if self
            .cache
            .direct_scan_worthwhile(&self.layers, self.active, region)
        {
            self.render_direct(region);
            return;
        }
        self.cache.begin_update();
        let serial_started = (crate::parallel_composite::timed_scan_candidate(region)
            && (region.width as u64 * region.height as u64 > 8 * 1024 * 1024
                || crate::parallel_composite::has_uncached_upper_layers(
                    &self.layers,
                    self.active,
                )))
        .then(std::time::Instant::now);
        // Bound temporary images, including each nested group's intermediate,
        // to 256 KiB instead of allocating another full canvas per group.
        const TILE: usize = 256;
        let mut ty = y;
        while ty < end_y {
            // Learn from completed rows even on the first composition. Keep
            // their pixels and cache; only the remaining rows change strategy.
            let remaining = Region::new(x, ty, end_x - x, end_y - ty);
            if ty != y
                && (self
                    .cache
                    .direct_scan_worthwhile(&self.layers, self.active, remaining)
                    || serial_started.is_some_and(|started| {
                        crate::parallel_composite::slow_scan_worthwhile(
                            remaining,
                            (ty - y) as u64 * (end_x - x) as u64,
                            started.elapsed(),
                        )
                    }))
            {
                self.render_direct(remaining);
                return;
            }
            // Probe medium previews with only 64 rows; a full 256-row prefix
            // would consume most of a typical curve before making a decision.
            let tile_height = if ty == y
                && serial_started.is_some()
                && region.width as u64 * region.height as u64 <= 8 * 1024 * 1024
            {
                64
            } else {
                TILE as u32
            };
            for tx in (x..end_x).step_by(TILE) {
                let w = (end_x - tx).min(TILE as u32);
                let h = (end_y - ty).min(tile_height);
                let mut region = PixelBuffer::transparent(w, h);
                self.cache.composite(
                    &self.layers,
                    None,
                    self.active,
                    &self.buffer,
                    Region::new(tx, ty, w, h),
                    &mut region,
                );
                self.composite.blit(&region, tx, ty);
            }
            ty += (end_y - ty).min(tile_height);
        }
    }

    fn render_direct(&mut self, region: Region) {
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8)
            .min(region.height as usize);
        crate::parallel_composite::render(
            &self.layers,
            self.active,
            &self.buffer,
            &mut self.composite,
            region,
            workers,
        );
        self.direct_scan_workers = workers;
    }

    /// The flattened image, ready to save or display.
    pub fn flattened(&mut self) -> &PixelBuffer {
        self.sync_active();
        self.composite_all();
        &self.composite
    }

    /// The flattened color at a canvas pixel.
    pub fn flattened_pixel(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || x >= self.width() as i32 || y >= self.height() as i32 {
            return None;
        }
        let mut out = PixelBuffer::transparent(1, 1);
        composite_region(
            &self.layers,
            self.active,
            &self.buffer,
            Region::new(x as u32, y as u32, 1, 1),
            &mut out,
        );
        out.get(0, 0)
    }

    /// A snapshot of the layer tree for undo/redo.
    pub fn history_snapshot(
        &mut self,
        previous: Option<&crate::HistorySnapshot>,
    ) -> crate::HistorySnapshot {
        self.sync_active();
        crate::HistorySnapshot {
            layers: crate::snapshot::capture_nodes(&self.layers, previous),
            active: self.active,
            next_id: self.next_id,
            layer_counter: self.layer_counter,
            group_counter: self.group_counter,
        }
    }

    /// Restores immutable history blocks to contiguous editable layers.
    pub fn restore_history(&mut self, snapshot: crate::HistorySnapshot) {
        self.sync_active();
        self.layers = crate::snapshot::restore_nodes(&snapshot.layers, &self.layers);
        self.active = snapshot.active;
        self.next_id = self.next_id.max(snapshot.next_id);
        self.layer_counter = snapshot.layer_counter;
        self.group_counter = snapshot.group_counter;
        self.refresh_buffer();
        self.composite_all();
    }

    /// A full-buffer snapshot, for callers that need editable layer pixels.
    pub fn layer_stack(&mut self) -> LayerStack {
        self.sync_active();
        LayerStack {
            layers: self.layers.clone(),
            active: self.active,
            next_id: self.next_id,
            layer_counter: self.layer_counter,
            group_counter: self.group_counter,
        }
    }

    /// Restores a snapshot taken by [`Self::layer_stack`].
    pub fn restore(&mut self, stack: LayerStack) {
        self.editing_active = None;
        self.layers = stack.layers;
        self.active = stack.active;
        // Ids only move forward, so a later branch never reuses the id of a
        // layer that a thumbnail cache may still remember.
        self.next_id = self.next_id.max(stack.next_id);
        self.layer_counter = stack.layer_counter;
        self.group_counter = stack.group_counter;
        self.refresh_buffer();
        self.composite_all();
    }

    /// Applies a pixel transform to every layer (canvas resize, crop, rotate).
    pub fn map_pixels(&mut self, mut f: impl FnMut(&PixelBuffer) -> PixelBuffer) {
        self.sync_active();
        map_layer_pixels(&mut self.layers, &mut f);
        self.refresh_buffer();
        self.composite_all();
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new(DEFAULT_CANVAS_WIDTH, DEFAULT_CANVAS_HEIGHT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{find_node, LayerBlend};

    #[test]
    fn separated_regions_handle_corner_proofs_and_bound_fragmentation() {
        let mut document = Document::new(512, 384);
        document.buffer.fill(Color::TRANSPARENT);
        document.buffer.set(0, 0, Color::BLACK);
        document.buffer.set(511, 383, Color::BLACK);
        let regions = document.layer_content_regions(1);
        assert_eq!(regions.len(), 2);
        assert_eq!(
            regions
                .iter()
                .map(|r| r.width as u64 * r.height as u64)
                .sum::<u64>(),
            2
        );
        document.buffer = PixelBuffer::transparent(4096, 256);
        for i in 0..41 {
            document
                .buffer
                .set(i % 32 * 128 + 2, i / 32 * 128 + 2, Color::BLACK);
        }
        let regions = document.layer_content_regions(1);
        assert_eq!(
            regions.len(),
            1,
            "fragmentation falls back to a bounded batch"
        );
        assert!(regions[0].width > 3900);
    }

    #[test]
    fn medium_preview_bands_match_reference_with_groups_and_clipped_edges() {
        let mut pixels = PixelBuffer::transparent(539, 409);
        for (i, p) in pixels.data_mut().chunks_exact_mut(4).enumerate() {
            p.copy_from_slice(&[i as u8, (i >> 8) as u8, (i * 73) as u8, 140]);
        }
        for mode in LayerBlend::ALL {
            let mut document = Document::new(539, 409);
            document.buffer = pixels.clone();
            let mut upper = Layer::new(2, "Upper", pixels.clone());
            upper.blend = mode;
            let mut group = crate::LayerGroup::new(3, "Group");
            group.opacity = 73;
            group.blend = mode;
            group.children.push(LayerNode::Layer(upper));
            group.children.push(document.layers.pop().unwrap());
            document.layers.push(LayerNode::Group(group));
            let sentinel = Color::rgb(120, 30, 70);
            document.composite.fill(sentinel);
            // Nonaligned origin, medium region and clamped right/bottom edges.
            let region = Region::new(7, 5, 532, 404);
            let mut expected = PixelBuffer::new(539, 409, sentinel);
            let mut tile = PixelBuffer::transparent(region.width, region.height);
            composite_region(
                &document.layers,
                document.active,
                &document.buffer,
                region,
                &mut tile,
            );
            expected.blit(&tile, region.x, region.y);
            document.composite_rect(7, 5, 600, 500);
            assert_eq!(document.composite, expected, "{mode:?}");
        }
    }

    #[test]
    fn default_uses_jspaint_dimensions() {
        let document = Document::default();
        assert_eq!(document.width(), DEFAULT_CANVAS_WIDTH);
        assert_eq!(document.height(), DEFAULT_CANVAS_HEIGHT);
        assert!(!document.transparent);
        assert_eq!(document.buffer.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn new_document_is_unmodified() {
        let document = Document::new(16, 16);
        assert!(!document.modified);
        assert_eq!(document.name, "Untitled");
    }

    #[test]
    fn starts_with_a_background_layer() {
        let document = Document::new(4, 4);
        assert_eq!(document.layers.len(), 1);
        assert_eq!(document.layers[0].name(), "Background");
        assert_eq!(document.active, document.layers[0].id());
        assert_eq!(document.layers[0].opacity(), 100);
        assert_eq!(document.layers[0].blend(), LayerBlend::Normal);
        assert!(find_node(&document.layers, document.active).is_some());
    }

    #[test]
    fn sync_active_and_restore_keep_the_working_buffer() {
        let mut document = Document::new(2, 2);
        document.buffer.set(1, 1, Color::BLACK);
        let stack = document.layer_stack();
        document.buffer.fill(Color::WHITE);
        document.restore(stack);
        assert_eq!(document.buffer.get(1, 1), Some(Color::BLACK));
        assert_eq!(document.composite.get(1, 1), Some(Color::BLACK));
    }

    #[test]
    fn flattened_pixel_composites_the_stack() {
        let mut document = Document::new(1, 1);
        let id = document.next_id;
        document.next_id += 1;
        let mut layer = Layer::new(id, "Top", PixelBuffer::new(1, 1, Color::BLACK));
        layer.opacity = 50;
        document.layers.insert(0, LayerNode::Layer(layer));
        assert_eq!(
            document.flattened_pixel(0, 0),
            Some(Color::rgb(128, 128, 128))
        );
    }

    #[test]
    fn editing_releases_the_active_mirror_and_syncs_before_tree_operations() {
        let mut document = Document::new(259, 131);
        document.composite_all();
        let address = document.buffer.data().as_ptr();
        document.prepare_active_edit();
        document.buffer.set(128, 128, Color::BLACK);
        assert_eq!(address, document.buffer.data().as_ptr());
        document.refresh_buffer();
        assert_eq!(document.buffer.get(128, 128), Some(Color::BLACK));
        document.composite_all();
        assert_eq!(document.composite.get(128, 128), Some(Color::BLACK));
        let snapshot = document.history_snapshot(None);
        assert_eq!(document.layers[0].pixel_size(), Some((259, 131)));
        document.prepare_active_edit();
        document.buffer.set(129, 128, Color::BLACK);
        document.restore_history(snapshot);
        assert_eq!(document.buffer.get(128, 128), Some(Color::BLACK));
        assert_eq!(document.buffer.get(129, 128), Some(Color::WHITE));
        document.layers.insert(
            0,
            LayerNode::Layer(Layer::new(2, "Other", PixelBuffer::transparent(259, 131))),
        );
        document.prepare_active_edit();
        document.buffer.set(129, 128, Color::BLACK);
        document.activate(2);
        assert_eq!(
            find_node(&document.layers, 1)
                .unwrap()
                .layer()
                .unwrap()
                .pixels
                .get(129, 128),
            Some(Color::BLACK)
        );
        assert_eq!(document.width(), 259);
    }

    #[test]
    fn tiled_composition_matches_full_with_nested_groups_and_partial_edges() {
        use crate::layer::LayerGroup;
        let mut document = Document::new(519, 273);
        document.buffer.set(257, 260, Color::BLACK);
        for mode in LayerBlend::ALL {
            let mut layer = Layer::new(
                2,
                "Top",
                PixelBuffer::new(519, 273, Color::rgba(20, 130, 210, 170)),
            );
            layer.blend = mode;
            let mut inner = LayerGroup::new(3, "Inner");
            inner.opacity = 65;
            inner.children.push(LayerNode::Layer(layer));
            let mut outer = LayerGroup::new(4, "Outer");
            outer.blend = mode;
            outer.children.push(LayerNode::Group(inner));
            document.layers.truncate(1);
            document.layers.insert(0, LayerNode::Group(outer));
            let mut expected = PixelBuffer::transparent(519, 273);
            composite_region(
                &document.layers,
                document.active,
                &document.buffer,
                Region::new(0, 0, 519, 273),
                &mut expected,
            );
            document.composite_all();
            assert_eq!(document.composite, expected, "full {mode:?}");
            document.composite.fill(Color::TRANSPARENT);
            document.composite_rect(7, 9, u32::MAX, u32::MAX);
            assert_eq!(
                document.composite.region_rgba(7, 9, 512, 264),
                expected.region_rgba(7, 9, 512, 264),
                "partial {mode:?}"
            );
            assert_eq!(document.composite.get(6, 9), Some(Color::TRANSPARENT));
            document.composite_rect(u32::MAX, u32::MAX, 10, 10);
        }
    }
}

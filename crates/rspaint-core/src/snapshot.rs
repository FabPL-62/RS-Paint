//! Immutable block-shared history, separate from contiguous drawing buffers.
use crate::{Layer, LayerBlend, LayerGroup, LayerNode, PixelBuffer};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use crate::buffer::HISTORY_BLOCK_SIZE as BLOCK;

#[derive(Clone, Debug)]
pub struct SnapshotPixels {
    width: u32,
    height: u32,
    blocks: Arc<Vec<Arc<Vec<u8>>>>,
    source: Weak<Vec<u8>>,
    versions: Arc<Vec<u64>>,
}

impl SnapshotPixels {
    fn capture(pixels: &PixelBuffer, previous: Option<&Self>) -> Self {
        let source = pixels.weak_data();
        let versions = pixels.history_versions();
        let previous =
            previous.filter(|p| p.width == pixels.width() && p.height == pixels.height());
        if let Some(old) = previous {
            if Weak::ptr_eq(&old.source, &source) {
                return old.clone();
            }
        }
        let cols = pixels.width().div_ceil(BLOCK);
        let rows = pixels.height().div_ceil(BLOCK);
        let mut blocks = Vec::with_capacity((cols * rows) as usize);
        for row in 0..rows {
            for col in 0..cols {
                let x = col * BLOCK;
                let y = row * BLOCK;
                let w = BLOCK.min(pixels.width() - x);
                let h = BLOCK.min(pixels.height() - y);
                let old = previous.map(|p| &p.blocks[(row * cols + col) as usize]);
                let index = (row * cols + col) as usize;
                let equal = previous.is_some_and(|p| p.versions[index] == versions[index])
                    || old.is_some_and(|old| {
                        old.chunks_exact(w as usize * 4)
                            .enumerate()
                            .all(|(dy, bytes)| {
                                let start =
                                    ((y as usize + dy) * pixels.width() as usize + x as usize) * 4;
                                bytes == &pixels.data()[start..start + bytes.len()]
                            })
                    });
                blocks.push(if equal {
                    old.unwrap().clone()
                } else {
                    Arc::new(pixels.region_rgba(x, y, w, h))
                });
            }
        }
        Self {
            width: pixels.width(),
            height: pixels.height(),
            blocks: Arc::new(blocks),
            source,
            versions,
        }
    }

    /// Unique allocation identities can be counted by slice address. Does not
    /// materialize a full image or count shared blocks more than once.
    pub fn allocations(&self) -> impl Iterator<Item = &[u8]> {
        self.blocks.iter().map(|block| block.as_slice())
    }

    fn restore(&self, current: Option<&PixelBuffer>) -> PixelBuffer {
        if let Some(current) = current {
            if self.width == current.width()
                && self.height == current.height()
                && (Weak::ptr_eq(&self.source, &current.weak_data()) || self.matches(current))
            {
                let mut out = current.clone();
                out.adopt_history_versions(self.versions.clone());
                return out;
            }
        }
        let mut out = PixelBuffer::transparent(self.width, self.height);
        let data = out.data_mut();
        let cols = self.width.div_ceil(BLOCK);
        for (index, block) in self.blocks.iter().enumerate() {
            let x = index as u32 % cols * BLOCK;
            let y = index as u32 / cols * BLOCK;
            let w = BLOCK.min(self.width - x) as usize;
            for (dy, bytes) in block.chunks_exact(w * 4).enumerate() {
                let start = ((y as usize + dy) * self.width as usize + x as usize) * 4;
                data[start..start + bytes.len()].copy_from_slice(bytes);
            }
        }
        out.adopt_history_versions(self.versions.clone());
        out
    }

    // A previously restored image has a new storage identity. Compare its
    // content before allocating again for a metadata-only undo/redo.
    fn matches(&self, current: &PixelBuffer) -> bool {
        let cols = self.width.div_ceil(BLOCK);
        self.blocks.iter().enumerate().all(|(index, block)| {
            let x = index as u32 % cols * BLOCK;
            let y = index as u32 / cols * BLOCK;
            let w = BLOCK.min(self.width - x) as usize;
            block.chunks_exact(w * 4).enumerate().all(|(dy, bytes)| {
                let start = ((y as usize + dy) * self.width as usize + x as usize) * 4;
                bytes == &current.data()[start..start + bytes.len()]
            })
        })
    }
}

#[derive(Clone, Debug)]
pub struct SnapshotLayer {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: u8,
    pub blend: LayerBlend,
    pub revision: u64,
    pub pixels: SnapshotPixels,
}

#[derive(Clone, Debug)]
pub struct SnapshotGroup {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: u8,
    pub blend: LayerBlend,
    pub expanded: bool,
    pub children: Vec<SnapshotNode>,
}

#[derive(Clone, Debug)]
pub enum SnapshotNode {
    Layer(SnapshotLayer),
    Group(SnapshotGroup),
}

impl SnapshotNode {
    pub fn children(&self) -> &[Self] {
        match self {
            Self::Layer(_) => &[],
            Self::Group(group) => &group.children,
        }
    }

    pub fn layer_pixels(&self) -> Option<&SnapshotPixels> {
        match self {
            Self::Layer(layer) => Some(&layer.pixels),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HistorySnapshot {
    pub layers: Vec<SnapshotNode>,
    pub active: u64,
    pub next_id: u64,
    pub layer_counter: u32,
    pub group_counter: u32,
}

fn index_layers<'a>(nodes: &'a [SnapshotNode], index: &mut HashMap<u64, &'a SnapshotPixels>) {
    for node in nodes {
        match node {
            SnapshotNode::Layer(layer) => {
                index.insert(layer.id, &layer.pixels);
            }
            SnapshotNode::Group(group) => index_layers(&group.children, index),
        }
    }
}

pub(crate) fn capture_nodes(
    nodes: &[LayerNode],
    previous: Option<&HistorySnapshot>,
) -> Vec<SnapshotNode> {
    let mut index = HashMap::new();
    if let Some(previous) = previous {
        index_layers(&previous.layers, &mut index);
    }
    fn capture(nodes: &[LayerNode], index: &HashMap<u64, &SnapshotPixels>) -> Vec<SnapshotNode> {
        nodes
            .iter()
            .map(|node| match node {
                LayerNode::Layer(layer) => SnapshotNode::Layer(SnapshotLayer {
                    id: layer.id,
                    name: layer.name.clone(),
                    visible: layer.visible,
                    locked: layer.locked,
                    opacity: layer.opacity,
                    blend: layer.blend,
                    revision: layer.revision,
                    pixels: SnapshotPixels::capture(&layer.pixels, index.get(&layer.id).copied()),
                }),
                LayerNode::Group(group) => SnapshotNode::Group(SnapshotGroup {
                    id: group.id,
                    name: group.name.clone(),
                    visible: group.visible,
                    locked: group.locked,
                    opacity: group.opacity,
                    blend: group.blend,
                    expanded: group.expanded,
                    children: capture(&group.children, index),
                }),
            })
            .collect()
    }
    capture(nodes, &index)
}

pub(crate) fn restore_nodes(nodes: &[SnapshotNode], current: &[LayerNode]) -> Vec<LayerNode> {
    fn index_layers<'a>(nodes: &'a [LayerNode], index: &mut HashMap<u64, &'a PixelBuffer>) {
        for node in nodes {
            match node {
                LayerNode::Layer(layer) => {
                    index.insert(layer.id, &layer.pixels);
                }
                LayerNode::Group(group) => index_layers(&group.children, index),
            }
        }
    }
    fn restore(nodes: &[SnapshotNode], current: &HashMap<u64, &PixelBuffer>) -> Vec<LayerNode> {
        nodes
            .iter()
            .map(|node| match node {
                SnapshotNode::Layer(layer) => LayerNode::Layer(Layer {
                    id: layer.id,
                    name: layer.name.clone(),
                    visible: layer.visible,
                    locked: layer.locked,
                    opacity: layer.opacity,
                    blend: layer.blend,
                    revision: layer.revision,
                    pixels: layer.pixels.restore(current.get(&layer.id).copied()),
                }),
                SnapshotNode::Group(group) => LayerNode::Group(LayerGroup {
                    id: group.id,
                    name: group.name.clone(),
                    visible: group.visible,
                    locked: group.locked,
                    opacity: group.opacity,
                    blend: group.blend,
                    expanded: group.expanded,
                    children: restore(&group.children, current),
                }),
            })
            .collect()
    }
    let mut index = HashMap::new();
    index_layers(current, &mut index);
    restore(nodes, &index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn snapshots_share_only_unchanged_blocks_and_preserve_edge_pixels() {
        let mut pixels = PixelBuffer::new(257, 129, Color::WHITE);
        let initial = SnapshotPixels::capture(&pixels, None);
        assert_eq!(initial.blocks.len(), 6);
        assert_eq!(
            initial.allocations().map(<[u8]>::len).sum::<usize>(),
            257 * 129 * 4
        );
        assert_eq!(
            initial.source.strong_count(),
            1,
            "history must not retain full source buffers"
        );
        let metadata = SnapshotPixels::capture(&pixels, Some(&initial));
        assert!(Arc::ptr_eq(&initial.blocks, &metadata.blocks));
        pixels.set(127, 127, Color::BLACK);
        pixels.set(128, 128, Color::rgba(5, 20, 70, 90));
        pixels.set(256, 128, Color::BLACK);
        let changed = SnapshotPixels::capture(&pixels, Some(&initial));
        for index in 0..6 {
            assert_eq!(
                Arc::ptr_eq(&initial.blocks[index], &changed.blocks[index]),
                ![0, 4, 5].contains(&index)
            );
        }
        assert_eq!(changed.restore(None), pixels);
        let restored = changed.restore(None);
        let reused = changed.restore(Some(&restored));
        assert!(Weak::ptr_eq(&restored.weak_data(), &reused.weak_data()));
        assert_eq!(
            initial.restore(None),
            PixelBuffer::new(257, 129, Color::WHITE)
        );
        // Even an untracked raw mutation with unchanged bytes reuses every block.
        pixels.data_mut()[0..4].copy_from_slice(&Color::WHITE.to_array());
        let unchanged = SnapshotPixels::capture(&pixels, Some(&changed));
        for (a, b) in changed.blocks.iter().zip(unchanged.blocks.iter()) {
            assert!(Arc::ptr_eq(a, b));
        }
        pixels.fill(Color::BLACK);
        let filled = SnapshotPixels::capture(&pixels, Some(&changed));
        assert_eq!(filled.restore(None), pixels);
        let resized = pixels.resized(129, 257, Color::TRANSPARENT);
        let resized_snapshot = SnapshotPixels::capture(&resized, Some(&filled));
        assert_eq!(resized_snapshot.restore(None), resized);
        assert_eq!(
            changed.restore(None).get(128, 128),
            Some(Color::rgba(5, 20, 70, 90))
        );
    }

    #[test]
    fn versioned_snapshots_match_pixels_through_raw_writes_copies_and_branches() {
        let mut pixels = PixelBuffer::new(257, 259, Color::WHITE);
        let mut old = SnapshotPixels::capture(&pixels, None);
        let mut snapshots = vec![old.clone()];
        let mut references = vec![pixels.clone()];
        for step in 0..48 {
            let color = Color::rgba(step * 3, step * 5, step * 2, 100 + step);
            match step % 8 {
                0 => pixels.set(128, 128, color),
                1 => pixels.row_mut(127, 127, 131).fill(step),
                2 => {
                    let len = pixels.data().len();
                    pixels.data_mut()[len - 1] = step;
                }
                3 => pixels.copy_region_from(
                    &PixelBuffer::new(pixels.width(), pixels.height(), color),
                    127,
                    127,
                    130,
                    131,
                ),
                4 => pixels.blit(&PixelBuffer::new(3, 3, color), 127, 127),
                5 => pixels.fill(color),
                6 => pixels = pixels.resized(259, 257, color).cropped(0, 0, 257, 257),
                _ => {
                    let branch = step as usize / 2;
                    pixels = snapshots[branch].restore(None);
                    pixels.set(127, 127, color);
                }
            }
            let new = SnapshotPixels::capture(&pixels, Some(&old));
            assert_eq!(new.restore(None), pixels, "step {step}");
            snapshots.push(new.clone());
            references.push(pixels.clone());
            old = new;
        }
        for (snapshot, reference) in snapshots.iter().zip(references.iter()) {
            assert_eq!(snapshot.restore(None), *reference);
        }
        let mut left = pixels.clone();
        let mut right = pixels;
        left.set(0, 0, Color::BLACK);
        right.set(0, 0, Color::rgb(1, 2, 3));
        let left_snapshot = SnapshotPixels::capture(&left, Some(&old));
        let right_snapshot = SnapshotPixels::capture(&right, Some(&left_snapshot));
        assert_eq!(right_snapshot.restore(None), right);
    }

    #[test]
    fn snapshot_tree_round_trip_keeps_all_layer_and_group_properties() {
        let mut leaf = Layer::new(
            7,
            "Nested",
            PixelBuffer::new(3, 2, Color::rgba(5, 20, 70, 90)),
        );
        leaf.visible = false;
        leaf.locked = true;
        leaf.opacity = 37;
        leaf.blend = LayerBlend::Multiply;
        leaf.revision = 101;
        let mut group = LayerGroup::new(8, "Group");
        group.visible = false;
        group.locked = true;
        group.opacity = 61;
        group.expanded = false;
        group.blend = LayerBlend::Dissolve;
        group.children.push(LayerNode::Layer(leaf));
        let nodes = vec![
            LayerNode::Group(group),
            LayerNode::Layer(Layer::new(9, "Base", PixelBuffer::new(3, 2, Color::WHITE))),
        ];
        let snapshot = capture_nodes(&nodes, None);
        let restored = restore_nodes(&snapshot, &[]);
        assert_eq!(format!("{nodes:?}"), format!("{restored:?}"));
        let reused = restore_nodes(&snapshot, &nodes);
        let base = nodes[1].layer().unwrap().pixels.weak_data();
        assert!(Weak::ptr_eq(
            &base,
            &reused[1].layer().unwrap().pixels.weak_data()
        ));
    }
}

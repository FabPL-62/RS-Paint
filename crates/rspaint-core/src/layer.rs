//! Layer stacks, blend modes and compositing.

use crate::buffer::PixelBuffer;
use crate::color::Color;

/// How a layer's pixels are combined with the layers below it.
///
/// The labels match the ones shown in the layer panel; the app localizes them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LayerBlend {
    #[default]
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
}

impl LayerBlend {
    /// All modes, in menu order.
    pub const ALL: [Self; 13] = [
        Self::Normal,
        Self::Dissolve,
        Self::Darken,
        Self::Multiply,
        Self::ColorBurn,
        Self::LinearBurn,
        Self::DarkerColor,
        Self::Lighten,
        Self::Screen,
        Self::ColorDodge,
        Self::LinearDodge,
        Self::LighterColor,
        Self::Overlay,
    ];

    /// The English display name (also the localization key).
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Dissolve => "Dissolve",
            Self::Darken => "Darken",
            Self::Multiply => "Multiply",
            Self::ColorBurn => "Color Burn",
            Self::LinearBurn => "Linear Burn",
            Self::DarkerColor => "Darker Color",
            Self::Lighten => "Lighten",
            Self::Screen => "Screen",
            Self::ColorDodge => "Color Dodge",
            Self::LinearDodge => "Linear Dodge (Add)",
            Self::LighterColor => "Lighter Color",
            Self::Overlay => "Overlay",
        }
    }

    /// The separable blend function, on straight (non-premultiplied) channels.
    fn blend_rgb(self, src: [f32; 3], dst: [f32; 3]) -> [f32; 3] {
        let per_channel =
            |f: fn(f32, f32) -> f32| [f(src[0], dst[0]), f(src[1], dst[1]), f(src[2], dst[2])];
        match self {
            Self::Normal | Self::Dissolve => src,
            Self::Darken => per_channel(f32::min),
            Self::Multiply => per_channel(|s, d| s * d),
            Self::ColorBurn => per_channel(|s, d| {
                if s <= 0.0 {
                    0.0
                } else {
                    1.0 - ((1.0 - d) / s).min(1.0)
                }
            }),
            Self::LinearBurn => per_channel(|s, d| (s + d - 1.0).max(0.0)),
            Self::DarkerColor => {
                if luminance(src) < luminance(dst) {
                    src
                } else {
                    dst
                }
            }
            Self::Lighten => per_channel(f32::max),
            Self::Screen => per_channel(|s, d| 1.0 - (1.0 - s) * (1.0 - d)),
            Self::ColorDodge => per_channel(|s, d| {
                if s >= 1.0 {
                    1.0
                } else {
                    (d / (1.0 - s)).min(1.0)
                }
            }),
            Self::LinearDodge => per_channel(|s, d| (s + d).min(1.0)),
            Self::LighterColor => {
                if luminance(src) > luminance(dst) {
                    src
                } else {
                    dst
                }
            }
            Self::Overlay => per_channel(|s, d| {
                if d <= 0.5 {
                    2.0 * s * d
                } else {
                    1.0 - 2.0 * (1.0 - s) * (1.0 - d)
                }
            }),
        }
    }
}

fn luminance(rgb: [f32; 3]) -> f32 {
    0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]
}

/// A stable 0..1 value per pixel, used by the Dissolve mode.
fn dissolve_sample(x: u32, y: u32) -> f32 {
    let mut hash = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xC2B2_AE3D);
    hash ^= hash >> 16;
    (hash & 0xFF) as f32 / 256.0
}

/// Composites `src` over `dst` with a blend mode and layer opacity.
fn blend_pixel(dst: Color, src: Color, opacity: f32, mode: LayerBlend, x: u32, y: u32) -> Color {
    let mut source_alpha = src.a as f32 / 255.0 * opacity;
    if source_alpha <= 0.0 {
        return dst;
    }
    let mode = if mode == LayerBlend::Dissolve {
        if dissolve_sample(x, y) >= source_alpha {
            return dst;
        }
        // Every surviving sample is fully opaque, so the overall coverage
        // matches the requested opacity.
        source_alpha = 1.0;
        LayerBlend::Normal
    } else {
        mode
    };

    let dst_alpha = dst.a as f32 / 255.0;
    if dst_alpha <= 0.0 {
        // Nothing underneath: the blend mode has no backdrop to act on.
        return Color::rgba(src.r, src.g, src.b, (source_alpha * 255.0).round() as u8);
    }

    let source = [
        src.r as f32 / 255.0,
        src.g as f32 / 255.0,
        src.b as f32 / 255.0,
    ];
    let backdrop = [
        dst.r as f32 / 255.0,
        dst.g as f32 / 255.0,
        dst.b as f32 / 255.0,
    ];
    let blended = mode.blend_rgb(source, backdrop);
    let out_alpha = source_alpha + dst_alpha * (1.0 - source_alpha);
    if out_alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let channel = |index: usize| {
        let color = source_alpha * (1.0 - dst_alpha) * source[index]
            + source_alpha * dst_alpha * blended[index]
            + (1.0 - source_alpha) * dst_alpha * backdrop[index];
        (color / out_alpha * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Color::rgba(
        channel(0),
        channel(1),
        channel(2),
        (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// A raster layer.
#[derive(Clone, Debug)]
pub struct Layer {
    pub id: u64,
    pub name: String,
    pub pixels: PixelBuffer,
    pub visible: bool,
    pub locked: bool,
    /// Opacity percentage, `0..=100`.
    pub opacity: u8,
    pub blend: LayerBlend,
    /// Bumped whenever `pixels` changes, for thumbnail caches.
    pub revision: u64,
}

impl Layer {
    pub fn new(id: u64, name: impl Into<String>, pixels: PixelBuffer) -> Self {
        Self {
            id,
            name: name.into(),
            pixels,
            visible: true,
            locked: false,
            opacity: 100,
            blend: LayerBlend::Normal,
            revision: 0,
        }
    }
}

/// A folder that groups layers. Its opacity and blend mode apply to the
/// composite of its children.
#[derive(Clone, Debug)]
pub struct LayerGroup {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: u8,
    pub blend: LayerBlend,
    pub expanded: bool,
    pub children: Vec<LayerNode>,
}

impl LayerGroup {
    pub fn new(id: u64, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            visible: true,
            locked: false,
            opacity: 100,
            blend: LayerBlend::Normal,
            expanded: true,
            children: Vec::new(),
        }
    }
}

/// One entry of a layer tree: a raster layer or a group folder.
#[derive(Clone, Debug)]
pub enum LayerNode {
    Layer(Layer),
    Group(LayerGroup),
}

impl LayerNode {
    pub fn id(&self) -> u64 {
        match self {
            Self::Layer(layer) => layer.id,
            Self::Group(group) => group.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Layer(layer) => &layer.name,
            Self::Group(group) => &group.name,
        }
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        match self {
            Self::Layer(layer) => layer.name = name.into(),
            Self::Group(group) => group.name = name.into(),
        }
    }

    pub fn visible(&self) -> bool {
        match self {
            Self::Layer(layer) => layer.visible,
            Self::Group(group) => group.visible,
        }
    }

    pub fn set_visible(&mut self, visible: bool) {
        match self {
            Self::Layer(layer) => layer.visible = visible,
            Self::Group(group) => group.visible = visible,
        }
    }

    pub fn locked(&self) -> bool {
        match self {
            Self::Layer(layer) => layer.locked,
            Self::Group(group) => group.locked,
        }
    }

    pub fn set_locked(&mut self, locked: bool) {
        match self {
            Self::Layer(layer) => layer.locked = locked,
            Self::Group(group) => group.locked = locked,
        }
    }

    pub fn opacity(&self) -> u8 {
        match self {
            Self::Layer(layer) => layer.opacity,
            Self::Group(group) => group.opacity,
        }
    }

    pub fn set_opacity(&mut self, opacity: u8) {
        match self {
            Self::Layer(layer) => layer.opacity = opacity.min(100),
            Self::Group(group) => group.opacity = opacity.min(100),
        }
    }

    pub fn blend(&self) -> LayerBlend {
        match self {
            Self::Layer(layer) => layer.blend,
            Self::Group(group) => group.blend,
        }
    }

    pub fn set_blend(&mut self, blend: LayerBlend) {
        match self {
            Self::Layer(layer) => layer.blend = blend,
            Self::Group(group) => group.blend = blend,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group(_))
    }

    pub fn layer(&self) -> Option<&Layer> {
        match self {
            Self::Layer(layer) => Some(layer),
            Self::Group(_) => None,
        }
    }

    pub fn layer_mut(&mut self) -> Option<&mut Layer> {
        match self {
            Self::Layer(layer) => Some(layer),
            Self::Group(_) => None,
        }
    }

    pub fn children(&self) -> &[LayerNode] {
        match self {
            Self::Layer(_) => &[],
            Self::Group(group) => &group.children,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<LayerNode>> {
        match self {
            Self::Layer(_) => None,
            Self::Group(group) => Some(&mut group.children),
        }
    }

    pub fn expanded(&self) -> bool {
        match self {
            Self::Layer(_) => false,
            Self::Group(group) => group.expanded,
        }
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        if let Self::Group(group) = self {
            group.expanded = expanded;
        }
    }

    pub fn revision(&self) -> u64 {
        match self {
            Self::Layer(layer) => layer.revision,
            Self::Group(_) => 0,
        }
    }

    /// The pixel size of this node, or of its first descendant layer.
    pub fn pixel_size(&self) -> Option<(u32, u32)> {
        match self {
            Self::Layer(layer) => Some((layer.pixels.width(), layer.pixels.height())),
            Self::Group(group) => group.children.iter().find_map(Self::pixel_size),
        }
    }
}

/// The undoable part of a layered document.
#[derive(Clone, Debug)]
pub struct LayerStack {
    /// Topmost entry first, as shown in the layer panel.
    pub layers: Vec<LayerNode>,
    pub active: u64,
    pub next_id: u64,
    pub layer_counter: u32,
    pub group_counter: u32,
}

/// A visible row of the layer tree, for the panel and range selections.
#[derive(Debug)]
pub struct LayerRow<'a> {
    pub node: &'a LayerNode,
    pub depth: usize,
    pub parent: Option<u64>,
}

/// Finds a node anywhere in the tree.
pub fn find_node(nodes: &[LayerNode], id: u64) -> Option<&LayerNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let Some(found) = find_node(node.children(), id) {
            return Some(found);
        }
    }
    None
}

/// Finds a node anywhere in the tree, mutably.
pub fn find_node_mut(nodes: &mut [LayerNode], id: u64) -> Option<&mut LayerNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let Some(children) = node.children_mut() {
            if let Some(found) = find_node_mut(children, id) {
                return Some(found);
            }
        }
    }
    None
}

/// The parent group of a node (`None` for a root node) and its index there.
pub fn parent_and_index(nodes: &[LayerNode], id: u64) -> Option<(Option<u64>, usize)> {
    if let Some(index) = nodes.iter().position(|node| node.id() == id) {
        return Some((None, index));
    }
    for node in nodes {
        if let (Some(parent), Some(mut found)) =
            (Some(node.id()), parent_and_index(node.children(), id))
        {
            found.0 = Some(parent);
            return Some(found);
        }
    }
    None
}

/// Whether `ancestor` is the same node as or above `id`.
pub fn is_ancestor(nodes: &[LayerNode], ancestor: u64, id: u64) -> bool {
    let Some(node) = find_node(nodes, ancestor) else {
        return false;
    };
    find_node(node.children(), id).is_some()
}

/// Removes a node (and its subtree), returning it when found.
pub fn remove_node(nodes: &mut Vec<LayerNode>, id: u64) -> Option<LayerNode> {
    if let Some(index) = nodes.iter().position(|node| node.id() == id) {
        return Some(nodes.remove(index));
    }
    for node in nodes.iter_mut() {
        if let Some(children) = node.children_mut() {
            if let Some(removed) = remove_node(children, id) {
                return Some(removed);
            }
        }
    }
    None
}

/// Inserts `node` into `parent` (or the root) at `index`, clamped to the end.
pub fn insert_node(nodes: &mut Vec<LayerNode>, parent: Option<u64>, index: usize, node: LayerNode) {
    if let Some(parent) = parent {
        if let Some(children) = find_node_mut(nodes, parent).and_then(LayerNode::children_mut) {
            let index = index.min(children.len());
            children.insert(index, node);
            return;
        }
        // The parent vanished: fall back to the root.
    }
    let index = index.min(nodes.len());
    nodes.insert(index, node);
}

/// The visible rows of the tree, topmost first. Collapsed groups hide their
/// children. Hidden layers stay listed.
pub fn visible_rows<'a>(nodes: &'a [LayerNode]) -> Vec<LayerRow<'a>> {
    let mut rows = Vec::new();
    collect_rows(nodes, 0, None, &mut rows);
    rows
}

fn collect_rows<'a>(
    nodes: &'a [LayerNode],
    depth: usize,
    parent: Option<u64>,
    rows: &mut Vec<LayerRow<'a>>,
) {
    for node in nodes {
        rows.push(LayerRow {
            node,
            depth,
            parent,
        });
        if node.expanded() {
            collect_rows(node.children(), depth + 1, Some(node.id()), rows);
        }
    }
}

/// Applies `f` to every layer's pixels, recursively.
pub fn map_layer_pixels(nodes: &mut [LayerNode], f: &mut impl FnMut(&PixelBuffer) -> PixelBuffer) {
    for node in nodes {
        match node {
            LayerNode::Layer(layer) => {
                layer.pixels = f(&layer.pixels);
                layer.revision = layer.revision.wrapping_add(1);
            }
            LayerNode::Group(group) => map_layer_pixels(&mut group.children, f),
        }
    }
}

/// A rectangular region of the canvas, in image coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Composites the tree into `out`, a region whose top-left is the region's
/// origin in canvas coordinates.
///
/// `active_pixels` overrides the stored pixels of the node `active`, because
/// the editor keeps the active layer in a working buffer that is only synced
/// back lazily.
pub fn composite_region(
    nodes: &[LayerNode],
    active: u64,
    active_pixels: &PixelBuffer,
    region: Region,
    out: &mut PixelBuffer,
) {
    for node in nodes.iter().rev() {
        match node {
            LayerNode::Layer(layer) if layer.visible && layer.opacity > 0 => {
                let pixels = if layer.id == active {
                    active_pixels
                } else {
                    &layer.pixels
                };
                blend_region(out, pixels, region.x, region.y, layer.opacity, layer.blend);
            }
            LayerNode::Group(group) if group.visible && group.opacity > 0 => {
                let mut inner = PixelBuffer::transparent(region.width, region.height);
                composite_region(&group.children, active, active_pixels, region, &mut inner);
                blend_region(out, &inner, 0, 0, group.opacity, group.blend);
            }
            _ => {}
        }
    }
}

/// Blends `src` (sampled at `src_x, src_y`) over the whole `out` region.
pub fn blend_region(
    out: &mut PixelBuffer,
    src: &PixelBuffer,
    src_x: u32,
    src_y: u32,
    opacity: u8,
    mode: LayerBlend,
) {
    let opacity = opacity as f32 / 100.0;
    let width = out.width().min(src.width().saturating_sub(src_x));
    let height = out.height().min(src.height().saturating_sub(src_y));
    for y in 0..height {
        for x in 0..width {
            let Some(source) = src.get(src_x + x, src_y + y) else {
                continue;
            };
            if source.a == 0 {
                continue;
            }
            let Some(dest) = out.get(x, y) else {
                continue;
            };
            out.set(x, y, blend_pixel(dest, source, opacity, mode, x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(buffer: &PixelBuffer) -> Color {
        buffer.get(0, 0).unwrap()
    }

    #[test]
    fn normal_blend_respects_layer_opacity() {
        let mut out = PixelBuffer::new(1, 1, Color::WHITE);
        let src = PixelBuffer::new(1, 1, Color::BLACK);
        blend_region(&mut out, &src, 0, 0, 50, LayerBlend::Normal);
        assert_eq!(pixel(&out), Color::rgb(128, 128, 128));
    }

    #[test]
    fn multiply_darkens_and_screen_lightens() {
        let mut out = PixelBuffer::new(1, 1, Color::rgb(128, 128, 128));
        let src = PixelBuffer::new(1, 1, Color::rgb(200, 200, 200));
        blend_region(&mut out, &src, 0, 0, 100, LayerBlend::Multiply);
        assert_eq!(pixel(&out), Color::rgb(100, 100, 100));

        let mut out = PixelBuffer::new(1, 1, Color::rgb(128, 128, 128));
        blend_region(&mut out, &src, 0, 0, 100, LayerBlend::Screen);
        assert_eq!(pixel(&out), Color::rgb(228, 228, 228));
    }

    #[test]
    fn hidden_layers_and_groups_do_not_composite() {
        let canvas = PixelBuffer::new(1, 1, Color::WHITE);
        let mut top = LayerNode::Layer(Layer::new(2, "Top", PixelBuffer::new(1, 1, Color::BLACK)));
        top.set_visible(false);
        let nodes = vec![
            top,
            LayerNode::Group(LayerGroup {
                id: 3,
                name: "Group".into(),
                visible: false,
                locked: false,
                opacity: 100,
                blend: LayerBlend::Normal,
                expanded: true,
                children: vec![LayerNode::Layer(Layer::new(
                    4,
                    "Inner",
                    PixelBuffer::new(1, 1, Color::BLACK),
                ))],
            }),
            LayerNode::Layer(Layer::new(1, "Background", canvas.clone())),
        ];
        let mut out = PixelBuffer::transparent(1, 1);
        composite_region(&nodes, 1, &canvas, Region::new(0, 0, 1, 1), &mut out);
        assert_eq!(pixel(&out), Color::WHITE);
    }

    #[test]
    fn group_opacity_applies_to_the_whole_group() {
        let canvas = PixelBuffer::new(1, 1, Color::WHITE);
        let nodes = vec![
            LayerNode::Group(LayerGroup {
                id: 2,
                name: "Group".into(),
                visible: true,
                locked: false,
                opacity: 50,
                blend: LayerBlend::Normal,
                expanded: true,
                children: vec![LayerNode::Layer(Layer::new(
                    3,
                    "Inner",
                    PixelBuffer::new(1, 1, Color::BLACK),
                ))],
            }),
            LayerNode::Layer(Layer::new(1, "Background", canvas.clone())),
        ];
        let mut out = PixelBuffer::transparent(1, 1);
        composite_region(&nodes, 1, &canvas, Region::new(0, 0, 1, 1), &mut out);
        assert_eq!(pixel(&out), Color::rgb(128, 128, 128));
    }

    #[test]
    fn tree_helpers_move_nodes_between_groups() {
        let mut nodes = vec![
            LayerNode::Layer(Layer::new(1, "A", PixelBuffer::transparent(1, 1))),
            LayerNode::Group(LayerGroup::new(2, "G")),
            LayerNode::Layer(Layer::new(3, "B", PixelBuffer::transparent(1, 1))),
        ];
        assert_eq!(parent_and_index(&nodes, 3), Some((None, 2)));
        assert!(!is_ancestor(&nodes, 2, 3));
        let removed = remove_node(&mut nodes, 3);
        assert!(removed.is_some());
        insert_node(
            &mut nodes,
            Some(2),
            0,
            LayerNode::Layer(Layer::new(3, "B", PixelBuffer::transparent(1, 1))),
        );
        assert_eq!(parent_and_index(&nodes, 3), Some((Some(2), 0)));
        assert!(is_ancestor(&nodes, 2, 3));
        let rows = visible_rows(&nodes);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].depth, 0);
        assert_eq!(rows[2].depth, 1);
    }
}

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

/// The state of a single image being edited.
///
/// The document is a stack of layers. `buffer` always holds the *active*
/// layer's working pixels (every editor operation reads and writes it), while
/// `composite` holds the flattened image shown on screen and saved to disk.
#[derive(Clone, Debug)]
pub struct Document {
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
    /// Creates a new opaque white document of the given size.
    pub fn new(width: u32, height: u32) -> Self {
        let buffer = PixelBuffer::new(width, height, Color::WHITE);
        let layers = vec![LayerNode::Layer(Layer::new(
            1,
            "Background",
            buffer.clone(),
        ))];
        Self {
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
    }

    /// Loads the active layer's pixels into the working buffer.
    pub fn refresh_buffer(&mut self) {
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
        let mut region = PixelBuffer::transparent(width, height);
        composite_region(
            &self.layers,
            self.active,
            &self.buffer,
            Region::new(x, y, width, height),
            &mut region,
        );
        self.composite.blit(&region, x, y);
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
}

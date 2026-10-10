//! Custom tool cursors, imported from JS Paint's `images/cursors`.
//!
//! egui has no API for image cursors, so the OS pointer is hidden over the
//! canvas and these images are drawn at the pointer position instead.

use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureOptions};

use crate::app::PickTarget;
use crate::tools::Tool;

/// A cursor image with its hotspot.
pub struct Cursor {
    texture: TextureHandle,
    /// Hotspot in image pixels (top-left origin).
    hotspot: egui::Vec2,
    /// Image size in pixels.
    size: egui::Vec2,
}

impl Cursor {
    /// Draws the cursor with its hotspot at `pointer` (screen coordinates).
    pub fn paint(&self, painter: &egui::Painter, pointer: egui::Pos2) {
        let rect = egui::Rect::from_min_size(pointer - self.hotspot, self.size);
        painter.image(
            self.texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

/// All loaded cursors, one texture per JS Paint cursor image.
pub struct CursorSet {
    precise: Cursor,
    precise_dotted: Cursor,
    pencil: Cursor,
    fill_bucket: Cursor,
    eye_dropper: Cursor,
    magnifier: Cursor,
    airbrush: Cursor,
    magic_wand: Cursor,
}

impl CursorSet {
    /// Loads the embedded cursor images as textures.
    pub fn load(ctx: &egui::Context) -> Self {
        Self {
            precise: load(
                ctx,
                "precise",
                include_bytes!("../assets/cursors/precise.png"),
                [16.0, 16.0],
            ),
            precise_dotted: load(
                ctx,
                "precise-dotted",
                include_bytes!("../assets/cursors/precise-dotted.png"),
                [16.0, 16.0],
            ),
            pencil: load(
                ctx,
                "pencil",
                include_bytes!("../assets/cursors/pencil.png"),
                [13.0, 23.0],
            ),
            fill_bucket: load(
                ctx,
                "fill-bucket",
                include_bytes!("../assets/cursors/fill-bucket.png"),
                [8.0, 22.0],
            ),
            eye_dropper: load(
                ctx,
                "eye-dropper",
                include_bytes!("../assets/cursors/eye-dropper.png"),
                [9.0, 22.0],
            ),
            magnifier: load(
                ctx,
                "magnifier",
                include_bytes!("../assets/cursors/magnifier.png"),
                [16.0, 16.0],
            ),
            airbrush: load(
                ctx,
                "airbrush",
                include_bytes!("../assets/cursors/airbrush.png"),
                [7.0, 22.0],
            ),
            magic_wand: load(
                ctx,
                "mwand",
                include_bytes!("../assets/cursors/mwand.png"),
                [10.0, 10.0],
            ),
        }
    }

    /// The cursor for the given tool, or the eyedropper while color-picking.
    pub fn for_state(&self, tool: Tool, picking: Option<PickTarget>) -> &Cursor {
        if picking.is_some() {
            return &self.eye_dropper;
        }
        match tool {
            Tool::Fill => &self.fill_bucket,
            Tool::PickColor => &self.eye_dropper,
            Tool::Magnifier => &self.magnifier,
            Tool::Pencil => &self.pencil,
            Tool::Airbrush => &self.airbrush,
            Tool::MagicWand => &self.magic_wand,
            Tool::Brush | Tool::SelectionBrush => &self.precise_dotted,
            _ => &self.precise,
        }
    }
}

/// Decodes an embedded PNG into a cursor texture.
fn load(ctx: &egui::Context, name: &str, bytes: &[u8], hotspot: [f32; 2]) -> Cursor {
    let image = image::load_from_memory(bytes)
        .expect("cursor image is valid")
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color_image = ColorImage::from_rgba_unmultiplied(size, &image.into_raw());
    let texture = ctx.load_texture(
        format!("cursor-{name}"),
        color_image,
        TextureOptions::NEAREST,
    );
    Cursor {
        texture,
        hotspot: egui::vec2(hotspot[0], hotspot[1]),
        size: egui::vec2(size[0] as f32, size[1] as f32),
    }
}

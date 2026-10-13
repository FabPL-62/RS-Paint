//! Embedded toolbar artwork, with nearest-neighbor sampling.
use crate::tools::Tool;
use eframe::egui;

#[derive(Clone)]
pub struct ToolIcons(Vec<egui::TextureHandle>);

pub fn description(tool: Tool) -> &'static str {
    match tool {
        Tool::FreeFormSelect => "Draw a freehand outline to select an irregular area. Shift adds; Alt subtracts.",
        Tool::Select => "Drag to select a rectangular area, then move or transform its pixels.",
        Tool::MagicWand => "Select connected pixels of a similar color. Adjust tolerance to include more shades.",
        Tool::SelectionBrush => "Paint the area you want to select. Alt removes pixels from the selection.",
        Tool::ContourSelect => "Select a rectangle while excluding pixels matching the secondary color.",
        Tool::Eraser => "Paint with the secondary color. Right-drag replaces only pixels of the primary color.",
        Tool::Fill => "Fill an area with the primary color. Hold Shift to replace matching colors throughout the image.",
        Tool::PickColor => "Sample a pixel: left-click sets the primary color; right-click sets the secondary color.",
        Tool::Magnifier => "Click to zoom in, right-click to zoom out, or drag a rectangle to zoom into that area.",
        Tool::Pencil => "Draw continuous pixel strokes with the primary color. Right-drag uses the secondary color.",
        Tool::Brush => "Paint with a circular, square or diagonal brush using the chosen size and blend mode.",
        Tool::Airbrush => "Spray color around the pointer. Adjust size to change the spray radius.",
        Tool::Text => "Drag a text box and type. Choose font, size, style and background transparency.",
        Tool::Line => "Drag to draw a straight line. Hold Shift to snap to multiples of 45 degrees.",
        Tool::Curve => "Drag the initial line, then drag twice to position the curve's control points.",
        Tool::Rectangle => "Drag to draw a rectangle with the chosen outline and fill. Shift makes a square.",
        Tool::Polygon => "Click to place vertices, then double-click or return to the first point to finish.",
        Tool::Ellipse => "Drag to draw an ellipse with the chosen outline and fill. Shift makes a circle.",
        Tool::RoundedRectangle => "Draw a rectangle with rounded corners. Adjust the radius in the tool options.",
        Tool::Outliner => "Select an area to add a border around its color contour using the primary color.",
    }
}

impl ToolIcons {
    pub fn load(ctx: &egui::Context) -> Self {
        let id = egui::Id::new("tool-icon-cache");
        if let Some(icons) = ctx.data(|data| data.get_temp::<Self>(id)) {
            return icons;
        }
        macro_rules! icons { ($($file:literal),+ $(,)?) => { [$(include_bytes!(concat!("../assets/tools/", $file, ".png")).as_slice()),+] }; }
        let bytes = icons![
            "select-freeform",
            "select",
            "magic-wand",
            "select-brush",
            "select-contour",
            "eraser",
            "fill",
            "pick",
            "magnify",
            "pencil",
            "brush",
            "airbrush",
            "text",
            "line",
            "curve",
            "rectangle",
            "polygon",
            "ellipse",
            "roundedr",
            "outliner"
        ];
        let icons = Self(
            bytes
                .iter()
                .zip(Tool::ALL)
                .map(|(bytes, tool)| {
                    let image = image::load_from_memory(bytes)
                        .expect("valid embedded tool icon")
                        .to_rgba8();
                    let size = [image.width() as usize, image.height() as usize];
                    ctx.load_texture(
                        format!("tool-{}", tool.name()),
                        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                        egui::TextureOptions::NEAREST,
                    )
                })
                .collect(),
        );
        ctx.data_mut(|data| data.insert_temp(id, icons.clone()));
        icons
    }
    pub fn texture(&self, tool: Tool) -> &egui::TextureHandle {
        &self.0[Tool::ALL
            .iter()
            .position(|&t| t == tool)
            .expect("tool is in catalog")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icons_are_cached_and_all_tools_have_translated_descriptions() {
        let ctx = egui::Context::default();
        let first = ToolIcons::load(&ctx);
        let second = ToolIcons::load(&ctx);
        for tool in Tool::ALL {
            assert_eq!(first.texture(tool).id(), second.texture(tool).id());
            assert!(first.texture(tool).size()[0] > 0);
            assert_ne!(crate::i18n::Locale::Spanish.text(tool.name()), tool.name());
            assert_ne!(
                crate::i18n::Locale::Spanish.text(description(tool)),
                description(tool)
            );
        }
    }
}

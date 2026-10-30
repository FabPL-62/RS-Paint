//! Application controller: OS integration and egui presentation.
use crate::canvas_renderer::{CanvasCallback, CanvasRenderer, Uniforms};
use crate::tools::Tool;
use crate::{theme, ui};
use eframe::{egui, egui_wgpu};
use egui::{Color32, Key, Modifiers, PointerButton};
use rspaint_core::{Color, History};
use rspaint_editor::Editor;
pub use rspaint_editor::{
    BorderPlacement, FlipRotate, PickTarget, ResizeHandle, SelectionOutline, ShapeFill, Shared,
    MAX_MAGNIFICATION, MIN_MAGNIFICATION,
};

pub fn to_color32(color: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}
pub(crate) fn pointer_button(button: PointerButton) -> rspaint_editor::PointerButton {
    match button {
        PointerButton::Primary => rspaint_editor::PointerButton::Primary,
        PointerButton::Secondary => rspaint_editor::PointerButton::Secondary,
        PointerButton::Middle => rspaint_editor::PointerButton::Middle,
        PointerButton::Extra1 => rspaint_editor::PointerButton::Extra1,
        PointerButton::Extra2 => rspaint_editor::PointerButton::Extra2,
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ColorTarget {
    Foreground,
    Background,
    /// A palette swatch, by index.
    Palette(usize),
    /// A captured palette swatch, by absolute index.
    Captured(usize),
    /// The Outliner's concave-corner color.
    Outliner,
}

/// The state of the advanced (HSV) color picker popup.
pub struct ColorPicker {
    /// Which color is being edited.
    pub target: ColorTarget,
    /// Hue in degrees (`0..360`).
    pub hue: f32,
    /// Saturation (`0..=1`).
    pub sat: f32,
    /// Value (`0..=1`).
    pub val: f32,
    /// Opacity (`0..=1`).
    pub alpha: f32,
    /// Where to place the popup, in screen coordinates.
    pub anchor: egui::Pos2,
    /// Set for the frame the popup opens, so the opening click doesn't close it.
    pub just_opened: bool,
}

/// Presentation state and adapters for a single document editor.
pub struct PaintApp {
    pub editor: Editor,
    pub tool_icons: crate::tool_icons::ToolIcons,
    canvas_upload: crate::canvas_renderer::PendingUpload,
    opened_document: Option<(rspaint_core::PixelBuffer, std::path::PathBuf)>,
    pub show_thumbnail: bool,
    pub show_bitmap: bool,
    pub show_custom_zoom: bool,
    pub custom_zoom_percent: f32,
    color_picker: Option<ColorPicker>,
    pub swatch_hold_fired: bool,
    pub captured_palette_row: usize,
    pub layers_ui: crate::ui::layers::LayersUiState,
    ctrl_down: bool,
    ctrl_tap_used: bool,
    pub show_history: bool,
    pub show_flip_rotate: bool,
    flip_rotate: FlipRotate,
    flip_rotate_angle: f64,
    pub show_stretch_skew: bool,
    stretch_x: f64,
    stretch_y: f64,
    skew_h: f64,
    skew_v: f64,
    pub show_about: bool,
    cursors: crate::cursors::CursorSet,
    pub show_attributes: bool,
    pub attributes_width: u32,
    pub attributes_height: u32,
    pub attributes_transparent: bool,
    pub error_message: Option<String>,
    last_title: String,
    thumbnail: Option<egui::TextureHandle>,
    thumbnail_revision: u64,
    has_gpu: bool,
    srgb_output: bool,
    window_id: u64,
    /// Whether this app hosts an empty window (no tabs): the menu and the tool
    /// buttons stay visible, but the canvas, the palette and the tool options
    /// are hidden.
    empty_host: bool,
    /// Set by File > New while hosting an empty window, so the workspace opens
    /// the new document in a tab instead of resetting the hidden one.
    new_tab_requested: bool,
}

impl PaintApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let mut has_gpu = false;
        let mut srgb_output = false;
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            srgb_output = render_state.target_format.is_srgb();
            let renderer = CanvasRenderer::new(&render_state.device, render_state.target_format);
            render_state
                .renderer
                .write()
                .callback_resources
                .insert(renderer);
            has_gpu = true;
        }

        Self::new_blank(has_gpu, srgb_output, &cc.egui_ctx)
    }

    pub fn new_blank(has_gpu: bool, srgb_output: bool, ctx: &egui::Context) -> Self {
        Self {
            editor: Editor::new(),
            tool_icons: crate::tool_icons::ToolIcons::load(ctx),
            canvas_upload: crate::canvas_renderer::PendingUpload::default(),
            opened_document: None,
            show_thumbnail: false,
            show_bitmap: false,
            show_custom_zoom: false,
            custom_zoom_percent: 100.0,
            color_picker: None,
            swatch_hold_fired: false,
            captured_palette_row: 0,
            layers_ui: Default::default(),
            ctrl_down: false,
            ctrl_tap_used: false,
            show_history: false,
            show_flip_rotate: false,
            flip_rotate: FlipRotate::FlipHorizontal,
            flip_rotate_angle: 0.0,
            show_stretch_skew: false,
            stretch_x: 100.0,
            stretch_y: 100.0,
            skew_h: 0.0,
            skew_v: 0.0,
            show_about: false,
            cursors: crate::cursors::CursorSet::load(ctx),
            show_attributes: false,
            attributes_width: rspaint_core::document::DEFAULT_CANVAS_WIDTH,
            attributes_height: rspaint_core::document::DEFAULT_CANVAS_HEIGHT,
            attributes_transparent: false,
            error_message: None,
            last_title: String::new(),
            thumbnail: None,
            thumbnail_revision: u64::MAX,
            has_gpu,
            srgb_output,
            window_id: 0,
            empty_host: false,
            new_tab_requested: false,
        }
    }

    pub fn has_gpu(&self) -> bool {
        self.has_gpu
    }

    /// Starts a new blank document (File > New).
    ///
    /// In a tab this replaces the document in place; in the empty window it
    /// asks the workspace for a new tab.
    pub fn new_document(&mut self) {
        if self.empty_host {
            self.new_tab_requested = true;
        } else {
            self.editor.reset_document();
        }
    }

    /// Takes the pending File > New request of an empty window's host app.
    pub(crate) fn take_new_tab_request(&mut self) -> bool {
        std::mem::take(&mut self.new_tab_requested)
    }

    pub fn empty_host(&self) -> bool {
        self.empty_host
    }

    /// Marks this app as the host of an empty window (no tabs).
    pub(crate) fn set_empty_host(&mut self, empty_host: bool) {
        self.empty_host = empty_host;
        self.new_tab_requested = false;
    }

    pub fn srgb_output(&self) -> bool {
        self.srgb_output
    }

    pub fn window_id(&self) -> u64 {
        self.window_id
    }

    pub fn set_window_id(&mut self, id: u64) {
        self.window_id = id;
    }

    pub fn document_name<'a>(&'a self, ctx: &egui::Context) -> &'a str {
        if self.editor.file_path.is_none() && self.editor.document.name == "Untitled" {
            crate::i18n::tr(ctx, "Untitled")
        } else {
            &self.editor.document.name
        }
    }

    pub fn tab_label(&self, ctx: &egui::Context) -> String {
        let marker = if self.editor.document.modified {
            "*"
        } else {
            ""
        };
        format!("{}{marker}", self.document_name(ctx))
    }

    pub fn open_color_picker(&mut self, target: ColorTarget, color: Color, anchor: egui::Pos2) {
        let (hue, sat, val) = color.to_hsv();
        self.color_picker = Some(ColorPicker {
            target,
            hue,
            sat,
            val,
            alpha: f32::from(color.a) / 255.0,
            anchor,
            just_opened: true,
        });
    }

    pub fn color_picker_ref(&self) -> Option<&ColorPicker> {
        self.color_picker.as_ref()
    }

    pub fn color_picker_mut(&mut self) -> Option<&mut ColorPicker> {
        self.color_picker.as_mut()
    }

    pub fn apply_color_picker(&mut self) {
        let Some(picker) = self.color_picker.as_ref() else {
            return;
        };
        let base = Color::from_hsv(picker.hue, picker.sat, picker.val);
        let alpha = (picker.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        let target = picker.target;
        let color = Color::rgba(base.r, base.g, base.b, alpha);
        self.editor.note_transparency(color);
        match target {
            ColorTarget::Foreground => self.editor.colors.foreground = color,
            ColorTarget::Background => self.editor.colors.background = color,
            ColorTarget::Palette(index) => {
                if let Some(slot) = self.editor.colors.palette.get_mut(index) {
                    *slot = color;
                }
            }
            ColorTarget::Captured(index) => {
                if let Some(slot) = self.editor.colors.captured.get_mut(index) {
                    *slot = color;
                }
            }
            ColorTarget::Outliner => self.editor.settings.outliner_color = color,
        }
    }

    pub fn close_color_picker(&mut self) {
        self.color_picker = None;
    }

    pub fn open_custom_zoom_dialog(&mut self) {
        self.custom_zoom_percent = (self.editor.magnification * 100.0).round();
        self.show_custom_zoom = true;
    }

    pub fn open_attributes_dialog(&mut self) {
        self.attributes_width = self.editor.document.width();
        self.attributes_height = self.editor.document.height();
        self.attributes_transparent = self.editor.document.transparent;
        self.show_attributes = true;
    }

    pub fn open_flip_rotate_dialog(&mut self) {
        self.show_flip_rotate = true;
    }

    pub fn open_stretch_skew_dialog(&mut self) {
        self.show_stretch_skew = true;
    }
    pub fn apply_attributes(&mut self) {
        self.editor.resize_document(
            self.attributes_width,
            self.attributes_height,
            self.attributes_transparent,
        );
        self.show_attributes = false;
    }
    pub fn apply_flip_rotate(&mut self) {
        self.editor
            .flip_rotate(self.flip_rotate, self.flip_rotate_angle);
        self.show_flip_rotate = false;
    }
    pub fn apply_stretch_skew(&mut self) {
        self.editor
            .stretch_skew(self.stretch_x, self.stretch_y, self.skew_h, self.skew_v);
        self.show_stretch_skew = false;
    }
}
/// File actions triggered by keyboard shortcuts.
enum FileAction {
    Save,
    SaveAs,
    Open,
}

fn bool_f32(value: bool) -> f32 {
    if value {
        1.0
    } else {
        0.0
    }
}

fn history_depth<T>(history: &History<T>, id: rspaint_core::NodeId) -> usize {
    let mut depth = 0;
    let mut current = id;
    while let Some(parent) = history.node(current).and_then(|node| node.parent) {
        depth += 1;
        current = parent;
    }
    depth
}

fn percent_row(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(1.0..=500.0).suffix("%"));
    });
}

fn angle_row(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(-89.0..=89.0).suffix("°"));
    });
}

fn key_for_char(c: char) -> Option<Key> {
    Some(match c.to_ascii_uppercase() {
        'A' => Key::A,
        'B' => Key::B,
        'C' => Key::C,
        'E' => Key::E,
        'F' => Key::F,
        'I' => Key::I,
        'L' => Key::L,
        'M' => Key::M,
        'O' => Key::O,
        'P' => Key::P,
        'R' => Key::R,
        'T' => Key::T,
        _ => return None,
    })
}
mod dialogs;
mod files;
mod input;
mod presentation;

//! Application state and the `eframe::App` implementation.

use eframe::egui;
use egui::{Color32, Key, Modifiers, PointerButton};
use rspaint_core::draw::{
    draw_bezier, draw_ellipse, draw_line, draw_quadratic_curve, draw_rectangle,
    draw_rounded_rectangle, erase_line, fill_ellipse, fill_rectangle, fill_rounded_rectangle,
    walk_dense_line, walk_line,
};
use rspaint_core::fill::flood_fill;
use rspaint_core::{palette, transform, Brush, BrushShape, Color, Document, History, PixelBuffer};

use crate::canvas_renderer::{CanvasRenderer, Uniforms, Upload};
use crate::tools::Tool;
use crate::{theme, ui};

/// Zoom limits for the View menu and the Ctrl+wheel zoom.
pub(crate) const MIN_MAGNIFICATION: f32 = 0.125;
pub(crate) const MAX_MAGNIFICATION: f32 = 64.0;
/// Largest corner radius for the Rounded Rectangle tool.
const MAX_ROUND_RADIUS: u32 = 200;

/// Converts a core color into an egui color.
pub fn to_color32(color: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

/// An axis-aligned, inclusive integer rectangle in image coordinates.
#[derive(Clone, Copy, Debug)]
struct RectI {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl RectI {
    /// The bounding box of two points.
    fn between(a: (i32, i32), b: (i32, i32)) -> Self {
        Self {
            x0: a.0.min(b.0),
            y0: a.1.min(b.1),
            x1: a.0.max(b.0),
            y1: a.1.max(b.1),
        }
    }

    /// Grows the rectangle by `margin` on every side.
    fn expand(self, margin: i32) -> Self {
        Self {
            x0: self.x0 - margin,
            y0: self.y0 - margin,
            x1: self.x1 + margin,
            y1: self.y1 + margin,
        }
    }

    /// The smallest rectangle containing both.
    fn union(self, other: Self) -> Self {
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// Clamps to a `width` x `height` image, returning `(x, y, w, h)` or `None` if empty.
    fn clamped(self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        let x0 = self.x0.max(0);
        let y0 = self.y0.max(0);
        let x1 = self.x1.min(width as i32 - 1);
        let y1 = self.y1.min(height as i32 - 1);
        if x0 > x1 || y0 > y1 {
            None
        } else {
            Some((
                x0 as u32,
                y0 as u32,
                (x1 - x0 + 1) as u32,
                (y1 - y0 + 1) as u32,
            ))
        }
    }

    /// Whether the rectangle spans the whole image.
    fn covers(self, width: u32, height: u32) -> bool {
        self.x0 <= 0 && self.y0 <= 0 && self.x1 >= width as i32 - 1 && self.y1 >= height as i32 - 1
    }
}

/// An in-progress drawing operation.
struct Stroke {
    color: Color,
    fill_color: Color,
    start: (i32, i32),
    last: (i32, i32),
    name: &'static str,
    /// Whether this stroke is a color eraser (right-drag with the Eraser).
    color_eraser: bool,
    /// Pre-stroke copy of the canvas, used by shape tools to redraw previews.
    base: Option<PixelBuffer>,
    /// The bounding box of the last shape preview, restored before the next one.
    preview_rect: Option<RectI>,
}

/// A horizontal selection highlight run `(y, x_start, x_end)`, inclusive.
pub type HighlightRun = (i32, i32, i32);

/// A floating selection: pixel content plus its position on the canvas.
#[derive(Clone)]
pub struct Selection {
    pixels: PixelBuffer,
    x: i32,
    y: i32,
    /// Polygon outline relative to the selection origin; empty for rectangles.
    outline: Vec<(i32, i32)>,
    /// Whether this is a pixel-mask selection (magic wand).
    mask: bool,
    /// Cached outline segments for a mask selection, relative to its origin.
    boundary: Vec<rspaint_core::selection::BoundarySegment>,
    /// Per-pixel selected area (bounding-box sized). Empty means "whole box".
    mask_bits: Vec<bool>,
    /// Cached horizontal runs `(y, x_start, x_end)` of the mask, relative to
    /// the origin, for drawing the selection highlight efficiently.
    runs: Vec<HighlightRun>,
    floating: bool,
    dragging: bool,
    moved: bool,
    drag_offset: (i32, i32),
    /// The canvas with the original region cleared, used while floating.
    base: Option<PixelBuffer>,
    name: &'static str,
}

/// A canvas edge/corner resize handle.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl ResizeHandle {
    /// All handles, in a stable order.
    pub const ALL: [ResizeHandle; 8] = [
        ResizeHandle::North,
        ResizeHandle::South,
        ResizeHandle::East,
        ResizeHandle::West,
        ResizeHandle::NorthEast,
        ResizeHandle::NorthWest,
        ResizeHandle::SouthEast,
        ResizeHandle::SouthWest,
    ];

    fn touches_east(self) -> bool {
        matches!(self, Self::East | Self::NorthEast | Self::SouthEast)
    }
    fn touches_west(self) -> bool {
        matches!(self, Self::West | Self::NorthWest | Self::SouthWest)
    }
    fn touches_north(self) -> bool {
        matches!(self, Self::North | Self::NorthWest | Self::NorthEast)
    }
    fn touches_south(self) -> bool {
        matches!(self, Self::South | Self::SouthWest | Self::SouthEast)
    }
}

/// An in-progress canvas resize drag.
struct ResizeDrag {
    handle: ResizeHandle,
    orig_buffer: PixelBuffer,
    orig_width: u32,
    orig_height: u32,
    /// The canvas rectangle's top-left at the start of the drag (screen space).
    start_min: egui::Pos2,
    magnification: f32,
    /// The selection origin at the start of the drag, so it can follow the
    /// content when the canvas grows from the top/left.
    sel_orig: Option<(i32, i32)>,
}

/// An in-progress resize of the selection content via its handles.
struct SelectionResizeDrag {
    handle: ResizeHandle,
    orig_pixels: PixelBuffer,
    orig_mask: Vec<bool>,
    orig_x: i32,
    orig_y: i32,
    orig_w: u32,
    orig_h: u32,
    /// Canvas with the selection region cleared.
    base: PixelBuffer,
    /// Selection box top-left at the start of the drag (screen space).
    start_min: egui::Pos2,
    magnification: f32,
}

/// Content copied to the internal clipboard for pasting.
#[derive(Clone)]
struct ClipboardContent {
    pixels: PixelBuffer,
    mask_bits: Vec<bool>,
    outline: Vec<(i32, i32)>,
    mask: bool,
}

/// A freshly created selection region before it is finalized.
struct NewRegion {
    pixels: PixelBuffer,
    x: i32,
    y: i32,
    mask_bits: Vec<bool>,
    outline: Vec<(i32, i32)>,
    /// Render as a pixel-mask boundary (magic wand) rather than an outline.
    mask: bool,
    name: &'static str,
}

/// A selection being dragged out but not yet finalized.
enum CreatingSelection {
    Rect { start: (i32, i32), end: (i32, i32) },
    Lasso { points: Vec<(i32, i32)> },
}

/// The on-canvas outline of a selection, used for the marching-ants overlay.
pub enum SelectionOutline {
    Rect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Poly(Vec<(i32, i32)>),
}

/// A tiny deterministic PRNG for the Airbrush tool.
#[derive(Clone, Debug)]
struct Rng(u32);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

/// An active text box being edited with the Text tool.
#[derive(Clone, Copy)]
pub struct TextBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub family: crate::text::FontFamily,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub vertical: bool,
    pub transparent: bool,
}

/// Which color a color picker edits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ColorTarget {
    Foreground,
    Background,
    /// A palette swatch, by index.
    Palette(usize),
    /// A custom (recent) color swatch, by index.
    Custom(usize),
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
    /// Where to place the popup, in screen coordinates.
    pub anchor: egui::Pos2,
    /// Set for the frame the popup opens, so the opening click doesn't close it.
    pub just_opened: bool,
}

/// Which active color a temporary Ctrl/Alt color-pick writes to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickTarget {
    Primary,
    Secondary,
}

/// The choice made in the close-confirmation dialog.
#[derive(Clone, Copy)]
enum Action {
    Cancel,
    SaveAndClose,
    Exit,
}

/// The root application state.
pub struct PaintApp {
    /// The image currently being edited.
    pub document: Document,
    /// The non-linear undo/redo history.
    pub history: History,
    /// The selected tool.
    pub tool: Tool,
    /// Left-click / stroke color.
    pub foreground: Color,
    /// Right-click / fill color.
    pub background: Color,
    /// The default palette swatches.
    pub palette: Vec<Color>,
    /// The customizable swatches.
    pub custom_colors: Vec<Color>,
    /// Brush tip shape for the Brush tool.
    pub brush_shape: BrushShape,
    /// Brush size for the Brush tool.
    pub brush_size: u32,
    /// Eraser size.
    pub eraser_size: u32,
    /// Eraser shape (circle or square).
    pub eraser_shape: BrushShape,
    /// Airbrush spray radius.
    pub airbrush_size: u32,
    /// Pencil size.
    pub pencil_size: u32,
    /// Outline thickness for line and shape tools.
    pub stroke_size: u32,
    /// Corner radius for the Rounded Rectangle tool (`/` and `*` keys).
    pub round_radius: u32,
    /// Whether shape tools fill their interior with the background color.
    pub fill_style: ShapeFill,
    /// Opacity of the pixel grid, as a percentage (0-100).
    pub grid_opacity: u32,
    /// Outliner: border thickness in pixels.
    pub outliner_size: u32,
    /// Outliner: whether to fill concave corners with the extra color.
    pub outliner_concave: bool,
    /// Outliner: extra color used for concave corners.
    pub outliner_color: Color,
    /// Outliner: where the border is drawn relative to the contour.
    pub outliner_placement: BorderPlacement,
    /// Current zoom factor (1.0 = 100%).
    pub magnification: f32,
    /// Pan offset of the canvas top-left relative to the viewport top-left.
    pub pan: egui::Vec2,
    /// Path of the currently open file, if any.
    pub file_path: Option<std::path::PathBuf>,
    /// The active selection, if any.
    pub selection: Option<Selection>,
    /// A selection currently being dragged out.
    creating_selection: Option<CreatingSelection>,
    /// Whether the in-progress selection should be added to the existing one.
    creating_selection_add: bool,
    /// Whether the in-progress selection should be subtracted from the existing one.
    creating_selection_subtract: bool,
    /// Undo stack of selection states (selection + canvas) for selection ops.
    selection_undo: Vec<(Option<Selection>, PixelBuffer)>,
    /// Content copied with Ctrl+C for pasting.
    clipboard: Option<ClipboardContent>,
    /// The in-progress canvas resize, if any.
    resize_drag: Option<ResizeDrag>,
    /// The in-progress selection-content resize, if any.
    selection_resize: Option<SelectionResizeDrag>,
    /// The size of the canvas viewport, updated each frame (for initial zoom).
    viewport_size: egui::Vec2,
    /// Whether to adjust the zoom on the first frame with a known viewport.
    pending_adjust_zoom: bool,
    /// Committed vertices of the in-progress Polygon.
    polygon_points: Vec<(i32, i32)>,
    /// The cursor position previewed as the next Polygon vertex.
    polygon_preview: Option<(i32, i32)>,
    /// Stroke color captured when the Polygon was started.
    polygon_stroke: Color,
    /// Fill color captured when the Polygon was started.
    polygon_fill: Color,
    /// Whether selections treat the background color as transparent.
    pub selection_transparent: bool,
    /// Color tolerance for the Magic Wand (0 = exact match).
    pub magic_wand_tolerance: u8,
    /// The seed point of the current Magic Wand selection.
    magic_wand_seed: Option<(i32, i32)>,
    /// Working mask for the Selection Brush (canvas-sized).
    sel_brush_mask: Vec<bool>,
    /// Whether the Selection Brush stroke is subtracting.
    sel_brush_subtract: bool,
    /// Whether a Selection Brush stroke is in progress.
    sel_brush_active: bool,
    /// The last point of the Selection Brush stroke.
    sel_brush_last: Option<(i32, i32)>,
    /// The Curve tool's points: [start, end, control1, control2].
    curve_points: Vec<(i32, i32)>,
    /// Pre-curve copy of the canvas, restored on each preview.
    curve_base: Option<PixelBuffer>,
    /// Whether a Curve drag is in progress.
    curve_dragging: bool,
    /// Loaded fonts for the Text tool.
    fonts: crate::text::FontSet,
    /// The active text box being edited, if any.
    text_box: Option<TextBox>,
    /// The editable text of the active box.
    text_buffer: String,
    /// The text box being created by dragging: `(origin, current)`.
    text_creating: Option<((i32, i32), (i32, i32))>,
    /// Text tool option: font family.
    pub text_family: crate::text::FontFamily,
    /// Text tool option: font size.
    pub text_size: f32,
    /// Text tool option: bold.
    pub text_bold: bool,
    /// Text tool option: italic.
    pub text_italic: bool,
    /// Text tool option: vertical orientation.
    pub text_vertical: bool,
    /// Text tool option: transparent background.
    pub text_transparent: bool,
    /// Pre-text copy of the canvas, restored on each live preview.
    text_base: Option<PixelBuffer>,
    /// The advanced color picker popup, if open.
    color_picker: Option<ColorPicker>,
    /// Whether a long-press on a palette swatch already opened the picker.
    pub swatch_hold_fired: bool,
    /// Next slot in the custom (recent) color row to write.
    custom_index: usize,
    /// Cached outline of the in-progress Selection Brush mask.
    sel_brush_boundary: Vec<rspaint_core::selection::BoundarySegment>,
    /// Cached highlight runs of the in-progress Selection Brush mask.
    sel_brush_runs: Vec<HighlightRun>,
    /// Whether the command (Ctrl/Cmd) key was held last frame.
    ctrl_down: bool,
    /// Whether the command key was combined with another input this hold.
    ctrl_tap_used: bool,
    /// Whether the History window is open.
    pub show_history: bool,
    /// Whether the Flip/Rotate dialog is open.
    pub show_flip_rotate: bool,
    /// The operation selected in the Flip/Rotate dialog.
    flip_rotate: FlipRotate,
    /// The angle used by the arbitrary rotation option.
    flip_rotate_angle: f64,
    /// Whether the Stretch/Skew dialog is open.
    pub show_stretch_skew: bool,
    /// Stretch Horizontal percentage.
    stretch_x: f64,
    /// Stretch Vertical percentage.
    stretch_y: f64,
    /// Skew Horizontal angle (degrees).
    skew_h: f64,
    /// Skew Vertical angle (degrees).
    skew_v: f64,
    /// Whether the About window is open.
    pub show_about: bool,
    /// Whether the close-confirmation dialog is open.
    show_close_confirm: bool,
    /// Set once the user confirmed closing, to bypass the confirmation.
    force_close: bool,
    /// Active temporary color-pick (Ctrl = primary, Alt = secondary).
    pick_hold: Option<PickTarget>,
    /// The loaded custom tool cursors.
    cursors: crate::cursors::CursorSet,
    /// Whether the Attributes dialog is open.
    pub show_attributes: bool,
    /// Attributes dialog: canvas width.
    pub attributes_width: u32,
    /// Attributes dialog: canvas height.
    pub attributes_height: u32,
    /// Attributes dialog: transparent mode.
    pub attributes_transparent: bool,
    /// A pending error message to show in a dialog.
    pub error_message: Option<String>,
    last_title: String,
    /// The pointer position in image coordinates, if over the canvas.
    pub pointer: Option<(i32, i32)>,
    stroke: Option<Stroke>,
    rng: Rng,
    /// Set when the whole texture must be re-uploaded.
    canvas_dirty: bool,
    /// Region changed since the last upload, used for partial texture updates.
    dirty_rect: Option<RectI>,
    /// Whether the custom wgpu renderer was initialized.
    has_gpu: bool,
    /// Whether the render target expects linear (sRGB) output.
    srgb_output: bool,
}

impl PaintApp {
    /// Creates the application and applies the default theme.
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

        let document = Document::default();
        let history = History::new(document.buffer.clone(), "New");
        Self {
            document,
            history,
            tool: Tool::Pencil,
            foreground: Color::BLACK,
            background: Color::WHITE,
            palette: palette::default_palette(),
            custom_colors: palette::default_custom_colors(),
            brush_shape: BrushShape::Circle,
            brush_size: 4,
            eraser_size: 8,
            eraser_shape: BrushShape::Square,
            airbrush_size: 9,
            pencil_size: 1,
            stroke_size: 1,
            round_radius: 10,
            fill_style: ShapeFill::Outline,
            grid_opacity: 20,
            outliner_size: 1,
            outliner_concave: false,
            outliner_color: Color::WHITE,
            outliner_placement: BorderPlacement::Outside,
            magnification: 1.0,
            pan: egui::Vec2::ZERO,
            file_path: None,
            selection: None,
            creating_selection: None,
            creating_selection_add: false,
            creating_selection_subtract: false,
            selection_undo: Vec::new(),
            clipboard: None,
            resize_drag: None,
            selection_resize: None,
            viewport_size: egui::Vec2::ZERO,
            pending_adjust_zoom: true,
            polygon_points: Vec::new(),
            polygon_preview: None,
            polygon_stroke: Color::BLACK,
            polygon_fill: Color::WHITE,
            selection_transparent: false,
            magic_wand_tolerance: 0,
            magic_wand_seed: None,
            sel_brush_mask: Vec::new(),
            sel_brush_subtract: false,
            sel_brush_active: false,
            sel_brush_last: None,
            sel_brush_boundary: Vec::new(),
            sel_brush_runs: Vec::new(),
            curve_points: Vec::new(),
            curve_base: None,
            curve_dragging: false,
            fonts: crate::text::FontSet::load(),
            text_box: None,
            text_buffer: String::new(),
            text_creating: None,
            text_family: crate::text::FontFamily::Sans,
            text_size: 16.0,
            text_bold: false,
            text_italic: false,
            text_vertical: false,
            text_transparent: true,
            text_base: None,
            color_picker: None,
            swatch_hold_fired: false,
            custom_index: 0,
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
            show_close_confirm: false,
            force_close: false,
            pick_hold: None,
            cursors: crate::cursors::CursorSet::load(&cc.egui_ctx),
            show_attributes: false,
            attributes_width: rspaint_core::document::DEFAULT_CANVAS_WIDTH,
            attributes_height: rspaint_core::document::DEFAULT_CANVAS_HEIGHT,
            attributes_transparent: false,
            error_message: None,
            last_title: String::new(),
            pointer: None,
            stroke: None,
            rng: Rng(0x1234_5678),
            canvas_dirty: true,
            dirty_rect: None,
            has_gpu,
            srgb_output,
        }
    }

    /// Whether the custom wgpu renderer is available.
    pub fn has_gpu(&self) -> bool {
        self.has_gpu
    }

    /// The size of the active tool's cursor in image pixels.
    pub fn cursor_size(&self) -> u32 {
        // A temporary color-pick uses a single-pixel cursor.
        if self.pick_hold.is_some() {
            return 1;
        }
        match self.tool {
            Tool::Pencil => self.pencil_size,
            Tool::Brush | Tool::SelectionBrush => self.brush_size,
            Tool::Eraser => self.eraser_size,
            // The airbrush sprays within `airbrush_size` pixels of the cursor.
            Tool::Airbrush => self.airbrush_size * 2 + 1,
            Tool::Line | Tool::Curve | Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle => {
                self.stroke_size
            }
            _ => 1,
        }
    }

    /// The shader code for the active tool's cursor footprint shape:
    /// 0 = square, 1 = circle, 2 = diagonal `\`, 3 = reverse diagonal `/`.
    fn cursor_shape_code(&self) -> f32 {
        if self.pick_hold.is_some() {
            return 0.0;
        }
        let code = match self.tool {
            Tool::Airbrush => 1,
            Tool::Brush | Tool::SelectionBrush => match self.brush_shape {
                BrushShape::Square => 0,
                BrushShape::Circle => 1,
                BrushShape::Diagonal => 2,
                BrushShape::ReverseDiagonal => 3,
            },
            Tool::Eraser => match self.eraser_shape {
                BrushShape::Circle => 1,
                _ => 0,
            },
            _ => 0,
        };
        code as f32
    }

    /// Replaces the document with a fresh, default one.
    pub fn reset_document(&mut self) {
        self.document = Document::default();
        self.history = History::new(self.document.buffer.clone(), "New");
        self.stroke = None;
        self.selection = None;
        self.creating_selection = None;
        self.selection_undo.clear();
        self.adjust_zoom();
        self.mark_full();
    }

    /// Clears the canvas to the background (or transparency).
    pub fn clear_image(&mut self) {
        let color = self.canvas_clear_color();
        self.document.buffer.fill(color);
        self.commit("Clear Image");
        self.mark_full();
    }

    /// Doubles the zoom factor.
    pub fn zoom_in(&mut self) {
        self.magnification = (self.magnification * 2.0).min(MAX_MAGNIFICATION);
    }

    /// Halves the zoom factor.
    pub fn zoom_out(&mut self) {
        self.magnification = (self.magnification / 2.0).max(MIN_MAGNIFICATION);
    }

    /// Undoes the last action, if possible.
    pub fn undo(&mut self) {
        // Undo the most recent selection operation first.
        if let Some((mut selection, buffer)) = self.selection_undo.pop() {
            if let Some(selection) = selection.as_mut() {
                selection.dragging = false;
            }
            self.document.buffer = buffer;
            self.selection = selection;
            self.creating_selection = None;
            self.creating_selection_add = false;
            self.creating_selection_subtract = false;
            self.mark_full();
            return;
        }
        if self.history.undo() {
            self.document.buffer = self.history.current_snapshot().clone();
            self.document.modified = true;
            self.mark_full();
        }
    }

    /// Redoes the last undone action, if possible.
    pub fn redo(&mut self) {
        self.selection_undo.clear();
        if self.history.redo() {
            self.document.buffer = self.history.current_snapshot().clone();
            self.document.modified = true;
            self.mark_full();
        }
    }

    /// Jumps to a specific state in the history tree.
    pub fn jump_to_history(&mut self, id: rspaint_core::NodeId) {
        if self.history.jump_to(id) {
            self.document.buffer = self.history.current_snapshot().clone();
            self.document.modified = true;
            self.mark_full();
        }
    }

    /// Records the current image as a new history state.
    pub fn commit(&mut self, name: &str) {
        self.history
            .commit(name.to_string(), self.document.buffer.clone());
        self.document.modified = true;
    }

    /// Updates the hovered pointer position in image coordinates.
    pub fn set_pointer(&mut self, pointer: Option<(i32, i32)>) {
        self.pointer = pointer;
    }

    /// Selects a tool, committing any active selection or polygon.
    pub fn set_tool(&mut self, tool: Tool) {
        if self.tool == Tool::Polygon && tool != Tool::Polygon {
            self.finish_polygon();
        }
        if self.tool == Tool::Curve && tool != Tool::Curve {
            self.curve_cancel();
        }
        if self.tool == Tool::Text && tool != Tool::Text {
            self.commit_text();
        }
        if !tool.is_selection() {
            self.deselect();
        }
        // Diagonal brush shapes only apply to the Brush tool.
        if tool == Tool::SelectionBrush
            && matches!(
                self.brush_shape,
                BrushShape::Diagonal | BrushShape::ReverseDiagonal
            )
        {
            self.brush_shape = BrushShape::Circle;
        }
        self.tool = tool;
    }

    /// The committed vertices of the in-progress polygon.
    pub fn polygon_points(&self) -> &[(i32, i32)] {
        &self.polygon_points
    }

    /// The previewed next vertex, if any.
    pub fn polygon_preview(&self) -> Option<(i32, i32)> {
        self.polygon_preview
    }

    /// Updates the previewed next vertex.
    pub fn set_polygon_preview(&mut self, preview: Option<(i32, i32)>) {
        self.polygon_preview = preview;
    }

    /// Handles a click while drawing a polygon.
    pub fn polygon_click(&mut self, p: (i32, i32)) {
        if self.polygon_points.len() >= 3 && self.polygon_near_start(p) {
            self.finish_polygon();
            return;
        }
        if self.polygon_points.is_empty() {
            self.polygon_stroke = self.foreground;
            self.polygon_fill = self.background;
        }
        self.polygon_points.push(p);
    }

    fn polygon_near_start(&self, p: (i32, i32)) -> bool {
        self.polygon_points.first().is_some_and(|&(x, y)| {
            let dx = p.0 - x;
            let dy = p.1 - y;
            dx * dx + dy * dy <= 16
        })
    }

    /// Finalizes the polygon, drawing it onto the canvas.
    pub fn finish_polygon(&mut self) {
        let points = std::mem::take(&mut self.polygon_points);
        self.polygon_preview = None;
        if points.len() < 2 {
            return;
        }
        let stroke = self.polygon_stroke;
        let fill = self.polygon_fill;
        self.note_used_color(stroke);
        let brush = Brush::new(BrushShape::Square, self.stroke_size);
        let interior = match self.fill_style {
            ShapeFill::Outline => None,
            ShapeFill::FilledWithOutline => Some(fill),
            ShapeFill::FilledOnly => Some(stroke),
        };
        if let Some(color) = interior {
            rspaint_core::draw::fill_polygon(&mut self.document.buffer, &points, color);
        }
        if self.fill_style != ShapeFill::FilledOnly {
            rspaint_core::draw::draw_polyline(
                &mut self.document.buffer,
                &points,
                &brush,
                stroke,
                true,
            );
        }
        self.mark_full();
        self.commit("Polygon");
    }

    /// Discards the in-progress polygon.
    pub fn cancel_polygon(&mut self) {
        self.polygon_points.clear();
        self.polygon_preview = None;
    }

    /// Begins (or extends) a Curve drag, adding a point at `p`.
    pub fn curve_pointer_down(&mut self, p: (i32, i32)) {
        if self.curve_points.is_empty() {
            self.curve_base = Some(self.document.buffer.clone());
            self.curve_points.push(p);
            self.curve_points.push(p);
        } else {
            self.curve_points.push(p);
        }
        self.curve_dragging = true;
        self.curve_redraw();
    }

    /// Moves the point currently being dragged.
    pub fn curve_pointer_move(&mut self, p: (i32, i32)) {
        if !self.curve_dragging {
            return;
        }
        if let Some(last) = self.curve_points.last_mut() {
            *last = p;
        }
        self.curve_redraw();
    }

    /// Ends a Curve drag, committing once the curve has four points.
    pub fn curve_pointer_up(&mut self) {
        self.curve_dragging = false;
        if self.curve_points.len() >= 4 {
            self.curve_base = None;
            self.curve_points.clear();
            self.note_used_color(self.foreground);
            self.commit("Curve");
        }
    }

    /// Discards the in-progress curve, restoring the canvas.
    pub fn curve_cancel(&mut self) {
        if self.curve_points.is_empty() {
            return;
        }
        if let Some(base) = self.curve_base.take() {
            self.document.buffer = base;
            self.mark_full();
        }
        self.curve_points.clear();
        self.curve_dragging = false;
    }

    fn curve_redraw(&mut self) {
        let Some(base) = self.curve_base.as_ref() else {
            return;
        };
        let (width, height) = (self.document.width(), self.document.height());
        self.document
            .buffer
            .copy_region_from(base, 0, 0, width, height);
        let brush = Brush::new(BrushShape::Square, self.stroke_size);
        let color = self.foreground;
        let points = self.curve_points.clone();
        let buffer = &mut self.document.buffer;
        match points.as_slice() {
            [] => {}
            [p] => brush.stamp(buffer, p.0, p.1, color),
            [start, end] => draw_line(buffer, *start, *end, &brush, color),
            [start, end, control] => {
                draw_quadratic_curve(buffer, *start, *control, *end, &brush, color)
            }
            [start, end, control_1, control_2] => {
                draw_bezier(buffer, *start, *control_1, *control_2, *end, &brush, color)
            }
            _ => {}
        }
        self.mark_full();
    }

    /// Whether a text box is currently being edited.
    pub fn has_text_box(&self) -> bool {
        self.text_box.is_some()
    }

    /// The active text box's geometry in image coordinates: `(x, y, width, height)`.
    pub fn text_box_rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.text_box.map(|b| (b.x, b.y, b.width, b.height))
    }

    /// Appends typed characters to the active box.
    pub fn text_type(&mut self, text: &str) {
        if self.text_box.is_some() {
            self.text_buffer.push_str(text);
        }
    }

    /// Deletes the last character of the active box.
    pub fn text_backspace(&mut self) {
        if self.text_box.is_some() {
            self.text_buffer.pop();
        }
    }

    /// Inserts a line break into the active box.
    pub fn text_newline(&mut self) {
        if self.text_box.is_some() {
            self.text_buffer.push('\n');
        }
    }

    /// The caret line (box-relative, in image pixels) at the end of the text.
    pub fn text_caret_segment(&self) -> Option<((f32, f32), (f32, f32))> {
        let text_box = self.text_box?;
        let style = crate::text::TextStyle {
            family: text_box.family,
            size: text_box.size,
            bold: text_box.bold,
            italic: text_box.italic,
        };
        if text_box.vertical {
            let (x, top, height) =
                crate::text::caret(&self.fonts, style, text_box.height, &self.text_buffer);
            let x0 = (text_box.width - 1 - top - height) as f32;
            let x1 = (text_box.width - 1 - top) as f32;
            let y = x as f32;
            Some(((x0, y), (x1, y)))
        } else {
            let (x, top, height) =
                crate::text::caret(&self.fonts, style, text_box.width, &self.text_buffer);
            let cx = x as f32;
            Some(((cx, top as f32), (cx, (top + height) as f32)))
        }
    }

    /// The rectangle being created by dragging, if any.
    pub fn text_creating_rect(&self) -> Option<((i32, i32), (i32, i32))> {
        self.text_creating
    }

    /// Starts creating a text box at `p`.
    pub fn begin_text_box(&mut self, p: (i32, i32)) {
        self.text_creating = Some((p, p));
    }

    /// Updates the text box being created.
    pub fn update_text_box(&mut self, p: (i32, i32)) {
        if let Some((_, end)) = self.text_creating.as_mut() {
            *end = p;
        }
    }

    /// Finishes creating a text box, opening it when large enough.
    pub fn finish_text_box(&mut self) {
        if let Some((a, b)) = self.text_creating.take() {
            let width = (a.0 - b.0).abs() + 1;
            let height = (a.1 - b.1).abs() + 1;
            if width > 1 && height > 1 {
                self.open_text_box(a.0.min(b.0), a.1.min(b.1), width, height);
            }
        }
    }

    /// Opens a default-sized text box at `p` (used for a plain click).
    pub fn default_text_box(&mut self, p: (i32, i32)) {
        let style = self.text_style();
        let height = crate::text::line_height(&self.fonts, style) + 8;
        self.open_text_box(p.0, p.1, 120, height);
    }

    fn open_text_box(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.text_buffer.clear();
        self.text_base = Some(self.document.buffer.clone());
        self.text_box = Some(TextBox {
            x,
            y,
            width,
            height,
            family: self.text_family,
            size: self.text_size,
            bold: self.text_bold,
            italic: self.text_italic,
            vertical: self.text_vertical,
            transparent: self.text_transparent,
        });
    }

    fn text_style(&self) -> crate::text::TextStyle {
        crate::text::TextStyle {
            family: self.text_family,
            size: self.text_size,
            bold: self.text_bold,
            italic: self.text_italic,
        }
    }

    /// Restores the pre-text canvas, then renders the current text on top.
    pub fn render_text_preview(&mut self) {
        let Some(text_box) = self.text_box else {
            return;
        };
        if let Some(base) = self.text_base.as_ref() {
            let (width, height) = (self.document.width(), self.document.height());
            self.document
                .buffer
                .copy_region_from(base, 0, 0, width, height);
        }
        if !self.text_buffer.trim().is_empty() {
            let text = self.text_buffer.clone();
            self.draw_text_box(text_box, &text);
        }
        self.mark_full();
    }

    /// Rasterizes `text` into the canvas within `text_box`.
    fn draw_text_box(&mut self, text_box: TextBox, text: &str) {
        if !text_box.transparent {
            fill_rectangle(
                &mut self.document.buffer,
                (text_box.x, text_box.y),
                (
                    text_box.x + text_box.width - 1,
                    text_box.y + text_box.height - 1,
                ),
                self.background,
            );
        }
        let style = crate::text::TextStyle {
            family: text_box.family,
            size: text_box.size,
            bold: text_box.bold,
            italic: text_box.italic,
        };
        let width = text_box.width.max(1) as u32;
        let height = text_box.height.max(1) as u32;
        if text_box.vertical {
            // Vertical text: lay it out along the box height, then rotate 90°.
            let mut temp = PixelBuffer::transparent(height, width);
            crate::text::draw_text(
                &self.fonts,
                &mut temp,
                (0, 0),
                text_box.height,
                text,
                style,
                self.foreground,
            );
            let rotated = crate::text::rotate_90_cw(&temp);
            crate::text::composite(&mut self.document.buffer, &rotated, text_box.x, text_box.y);
        } else {
            let mut temp = PixelBuffer::transparent(width, height);
            crate::text::draw_text(
                &self.fonts,
                &mut temp,
                (0, 0),
                text_box.width,
                text,
                style,
                self.foreground,
            );
            crate::text::composite(&mut self.document.buffer, &temp, text_box.x, text_box.y);
        }
    }

    /// Commits the active text box onto the canvas.
    pub fn commit_text(&mut self) {
        let Some(text_box) = self.text_box.take() else {
            return;
        };
        let text = std::mem::take(&mut self.text_buffer);
        if let Some(base) = self.text_base.take() {
            let (width, height) = (self.document.width(), self.document.height());
            self.document
                .buffer
                .copy_region_from(&base, 0, 0, width, height);
        }
        if !text.trim().is_empty() {
            self.draw_text_box(text_box, &text);
            self.note_used_color(self.foreground);
            self.mark_full();
            self.commit("Text");
        } else {
            self.mark_full();
        }
    }

    /// Opens the advanced color picker for `target`, initialized to `color`.
    pub fn open_color_picker(&mut self, target: ColorTarget, color: Color, anchor: egui::Pos2) {
        let (hue, sat, val) = color.to_hsv();
        self.color_picker = Some(ColorPicker {
            target,
            hue,
            sat,
            val,
            anchor,
            just_opened: true,
        });
    }

    /// Read-only access to the open color picker.
    pub fn color_picker_ref(&self) -> Option<&ColorPicker> {
        self.color_picker.as_ref()
    }

    /// Mutable access to the open color picker.
    pub fn color_picker_mut(&mut self) -> Option<&mut ColorPicker> {
        self.color_picker.as_mut()
    }

    /// Applies the picker's color to its target.
    pub fn apply_color_picker(&mut self) {
        let Some(picker) = self.color_picker.as_ref() else {
            return;
        };
        let color = Color::from_hsv(picker.hue, picker.sat, picker.val);
        let target = picker.target;
        match target {
            ColorTarget::Foreground => self.foreground = color,
            ColorTarget::Background => self.background = color,
            ColorTarget::Palette(index) => {
                if let Some(slot) = self.palette.get_mut(index) {
                    *slot = color;
                }
            }
            ColorTarget::Custom(index) => {
                if let Some(slot) = self.custom_colors.get_mut(index) {
                    *slot = color;
                }
            }
            ColorTarget::Outliner => self.outliner_color = color,
        }
    }

    /// Closes the color picker.
    pub fn close_color_picker(&mut self) {
        self.color_picker = None;
    }

    /// Swaps the foreground and background colors.
    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.foreground, &mut self.background);
    }

    /// Records a color as recently used, filling the custom row left to right.
    ///
    /// Colors already present in the palette or the custom row are skipped.
    pub fn note_used_color(&mut self, color: Color) {
        let count = self.custom_colors.len();
        if count == 0 {
            return;
        }
        if self.palette.contains(&color) || self.custom_colors.contains(&color) {
            return;
        }
        self.custom_colors[self.custom_index % count] = color;
        self.custom_index = (self.custom_index + 1) % count;
    }

    /// Restores the default palette (the first two rows only).
    pub fn reset_colors(&mut self) {
        self.palette = palette::default_palette();
    }

    /// Captures distinct colors from the image into the custom row.
    ///
    /// Colors already in the palette are ignored; only the first
    /// [`CUSTOM_COLOR_COUNT`](rspaint_core::palette::CUSTOM_COLOR_COUNT)
    /// detected colors are kept, in scan order.
    pub fn capture_colors(&mut self) {
        let count = rspaint_core::palette::CUSTOM_COLOR_COUNT;
        let mut found: Vec<Color> = Vec::new();
        'scan: for y in 0..self.document.height() {
            for x in 0..self.document.width() {
                let Some(color) = self.document.buffer.get(x, y) else {
                    continue;
                };
                if color.a == 0 || self.palette.contains(&color) || found.contains(&color) {
                    continue;
                }
                found.push(color);
                if found.len() >= count {
                    break 'scan;
                }
            }
        }
        let mut row = vec![Color::BLACK; count];
        for (slot, color) in row.iter_mut().zip(&found) {
            *slot = *color;
        }
        self.custom_colors = row;
        self.custom_index = found.len() % count;
    }

    /// Discards the active text box and any in-progress box creation.
    pub fn cancel_text(&mut self) {
        if let Some(base) = self.text_base.take() {
            let (width, height) = (self.document.width(), self.document.height());
            self.document
                .buffer
                .copy_region_from(&base, 0, 0, width, height);
        }
        self.text_box = None;
        self.text_buffer.clear();
        self.text_creating = None;
        self.mark_full();
    }

    /// The outline of the active selection in canvas coordinates.
    pub fn selection_outline(&self) -> Option<SelectionOutline> {
        let selection = self.selection.as_ref()?;
        if selection.mask {
            return None;
        }
        if selection.outline.is_empty() {
            Some(SelectionOutline::Rect {
                x: selection.x,
                y: selection.y,
                width: selection.pixels.width(),
                height: selection.pixels.height(),
            })
        } else {
            Some(SelectionOutline::Poly(
                selection
                    .outline
                    .iter()
                    .map(|&(ox, oy)| (selection.x + ox, selection.y + oy))
                    .collect(),
            ))
        }
    }

    /// The outline of the in-progress selection, if any.
    pub fn creating_outline(&self) -> Option<SelectionOutline> {
        match self.creating_selection.as_ref()? {
            CreatingSelection::Rect { start, end } => {
                let x = start.0.min(end.0);
                let y = start.1.min(end.1);
                Some(SelectionOutline::Rect {
                    x,
                    y,
                    width: (start.0.max(end.0) - x + 1) as u32,
                    height: (start.1.max(end.1) - y + 1) as u32,
                })
            }
            CreatingSelection::Lasso { points } => Some(SelectionOutline::Poly(points.clone())),
        }
    }

    /// The active magic-wand selection's outline segments and origin.
    pub fn selection_boundary(
        &self,
    ) -> Option<(&[rspaint_core::selection::BoundarySegment], (i32, i32))> {
        let selection = self.selection.as_ref()?;
        selection
            .mask
            .then_some((selection.boundary.as_slice(), (selection.x, selection.y)))
    }

    /// Re-runs the magic wand with the current tolerance (used when it changes).
    pub fn reselect_magic_wand(&mut self) {
        if self.selection.is_some() {
            if let Some(seed) = self.magic_wand_seed {
                self.magic_wand_select(seed, false, false);
            }
        }
    }

    /// Selects the connected region of matching pixels at `p` (magic wand).
    ///
    /// When `add` is true, the region is added to the existing wand selection.
    pub fn magic_wand_select(&mut self, p: (i32, i32), add: bool, subtract: bool) {
        self.magic_wand_seed = Some(p);
        let Some((pixels, (x, y), mask_bits)) = rspaint_core::selection::magic_wand(
            &self.document.buffer,
            p.0,
            p.1,
            true,
            self.magic_wand_tolerance,
        ) else {
            return;
        };
        let region = NewRegion {
            pixels,
            x,
            y,
            mask_bits,
            outline: Vec::new(),
            mask: true,
            name: "Magic Wand",
        };
        self.combine_new_region(region, add, subtract);
    }

    /// Finalizes a new region, replacing or (when `add`) unioning the selection.
    fn combine_new_region(&mut self, region: NewRegion, add: bool, subtract: bool) {
        if subtract {
            let Some(previous) = self.selection.as_ref() else {
                return;
            };
            if !region_removes_any(previous, &region) {
                return;
            }
            self.selection_undo
                .push((self.selection.clone(), self.document.buffer.clone()));
            let previous = self.selection.take().expect("checked above");
            let was_floating = previous.floating;
            let base = previous.base.clone();
            match subtract_region(previous, &region) {
                Some(result) => {
                    if result.floating {
                        if let Some(base) = result.base.clone() {
                            self.document.buffer = base;
                            let background = if self.selection_transparent {
                                Some(self.background)
                            } else {
                                None
                            };
                            rspaint_core::selection::stamp(
                                &mut self.document.buffer,
                                &result.pixels,
                                result.x,
                                result.y,
                                background,
                            );
                        }
                        self.mark_full();
                    }
                    self.selection = Some(result);
                }
                None => {
                    // Nothing left selected; put back the floating pixels.
                    if was_floating {
                        if let Some(base) = base {
                            self.document.buffer = base;
                            self.mark_full();
                        }
                    }
                    self.selection = None;
                }
            }
            return;
        }

        if add {
            // Ignore a region that adds no pixels outside the current selection.
            if let Some(previous) = self.selection.as_ref() {
                if !region_adds_new(previous, &region) {
                    return;
                }
            }
            self.selection_undo
                .push((self.selection.clone(), self.document.buffer.clone()));
            if let Some(previous) = self.selection.take() {
                let fill = self.selection_fill();
                let union = union_region(previous, region, fill);
                if union.floating {
                    if let Some(base) = union.base.clone() {
                        self.document.buffer = base;
                        let background = if self.selection_transparent {
                            Some(self.background)
                        } else {
                            None
                        };
                        rspaint_core::selection::stamp(
                            &mut self.document.buffer,
                            &union.pixels,
                            union.x,
                            union.y,
                            background,
                        );
                    }
                    self.mark_full();
                }
                self.selection = Some(union);
                return;
            }
            self.selection = Some(selection_from_region(region));
        } else {
            self.deselect();
            self.selection_undo
                .push((self.selection.clone(), self.document.buffer.clone()));
            self.selection = Some(selection_from_region(region));
        }
    }

    /// Adds (Shift) or subtracts (Alt) a single clicked pixel.
    pub fn select_pixel(&mut self, p: (i32, i32), add: bool, subtract: bool) {
        if p.0 < 0
            || p.1 < 0
            || p.0 >= self.document.width() as i32
            || p.1 >= self.document.height() as i32
        {
            return;
        }
        let pixels = rspaint_core::selection::extract_rect(&self.document.buffer, p.0, p.1, 1, 1);
        let region = NewRegion {
            pixels,
            x: p.0,
            y: p.1,
            mask_bits: vec![true],
            outline: Vec::new(),
            mask: true,
            name: "Select",
        };
        self.combine_new_region(region, add, subtract);
    }

    /// Removes the connected selected region (contour) under `p` (Contour Select).
    pub fn contour_deselect_at(&mut self, p: (i32, i32)) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let (width, height) = (selection.pixels.width(), selection.pixels.height());
        let local_x = p.0 - selection.x;
        let local_y = p.1 - selection.y;
        if local_x < 0 || local_y < 0 || local_x as u32 >= width || local_y as u32 >= height {
            return;
        }
        let index = (local_y as u32 * width + local_x as u32) as usize;
        if !selection.mask_bits.get(index).copied().unwrap_or(false) {
            return;
        }
        self.selection_undo
            .push((self.selection.clone(), self.document.buffer.clone()));

        // Flood-fill the connected component (4-connected) and remove it.
        let mut mask_bits = self
            .selection
            .as_ref()
            .map(|selection| selection.mask_bits.clone())
            .unwrap_or_default();
        let mut stack = vec![(local_x, local_y)];
        let mut removed = Vec::new();
        while let Some((x, y)) = stack.pop() {
            if x < 0 || y < 0 || x as u32 >= width || y as u32 >= height {
                continue;
            }
            let index = (y as u32 * width + x as u32) as usize;
            if !mask_bits[index] {
                continue;
            }
            mask_bits[index] = false;
            removed.push((x as u32, y as u32));
            stack.push((x + 1, y));
            stack.push((x - 1, y));
            stack.push((x, y + 1));
            stack.push((x, y - 1));
        }

        if mask_bits.iter().all(|&selected| !selected) {
            self.selection = None;
            self.mark_full();
            return;
        }

        if let Some(selection) = self.selection.as_mut() {
            for (x, y) in removed {
                selection.pixels.set(x, y, Color::TRANSPARENT);
            }
            let is_rect = mask_bits.iter().all(|&selected| selected);
            selection.boundary =
                rspaint_core::selection::boundary_segments(&mask_bits, width, height);
            selection.runs = runs_from_mask(&mask_bits, width, height);
            selection.mask_bits = mask_bits;
            selection.mask = !is_rect;
            selection.outline = Vec::new();
            selection.floating = false;
            selection.base = None;
            selection.moved = false;
        }
        self.mark_full();
    }

    /// Begins a Selection Brush stroke.
    ///
    /// `add` (Shift) and `subtract` (right/Alt) keep the existing selection;
    /// otherwise the stroke replaces it.
    pub fn selection_brush_begin(&mut self, p: (i32, i32), subtract: bool, add: bool) {
        self.selection_undo
            .push((self.selection.clone(), self.document.buffer.clone()));

        let (width, height) = (self.document.width(), self.document.height());
        let mut mask = vec![false; (width * height) as usize];
        if add || subtract {
            if let Some(selection) = self.selection.as_ref() {
                let (sx, sy) = (selection.x, selection.y);
                for oy in 0..selection.pixels.height() as i32 {
                    for ox in 0..selection.pixels.width() as i32 {
                        let index = (oy as u32 * selection.pixels.width() + ox as u32) as usize;
                        if selection.mask_bits.get(index).copied().unwrap_or(true) {
                            let cx = sx + ox;
                            let cy = sy + oy;
                            if cx >= 0 && cy >= 0 && (cx as u32) < width && (cy as u32) < height {
                                mask[(cy as u32 * width + cx as u32) as usize] = true;
                            }
                        }
                    }
                }
            }
        }
        self.sel_brush_mask = mask;
        self.sel_brush_subtract = subtract;
        self.sel_brush_active = true;
        self.sel_brush_last = Some(p);
        self.selection_brush_stamp(p, p);
    }

    /// Continues a Selection Brush stroke.
    pub fn selection_brush_to(&mut self, p: (i32, i32)) {
        if !self.sel_brush_active {
            return;
        }
        if let Some(last) = self.sel_brush_last {
            self.selection_brush_stamp(last, p);
        }
        self.sel_brush_last = Some(p);
    }

    /// Finishes a Selection Brush stroke, building the selection from the mask.
    pub fn selection_brush_end(&mut self) {
        if !self.sel_brush_active {
            return;
        }
        self.sel_brush_active = false;
        self.sel_brush_last = None;
        self.sel_brush_boundary.clear();
        self.sel_brush_runs.clear();

        let (width, height) = (self.document.width(), self.document.height());
        let mask = std::mem::take(&mut self.sel_brush_mask);
        let mut min_x = width as i32;
        let mut min_y = height as i32;
        let mut max_x = -1;
        let mut max_y = -1;
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                if mask[(y as u32 * width + x as u32) as usize] {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if max_x < min_x {
            self.selection = None;
            return;
        }
        let box_w = (max_x - min_x + 1) as u32;
        let box_h = (max_y - min_y + 1) as u32;
        let mut content = PixelBuffer::transparent(box_w, box_h);
        let mut box_mask = vec![false; (box_w * box_h) as usize];
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if mask[(y as u32 * width + x as u32) as usize] {
                    box_mask[((y - min_y) as u32 * box_w + (x - min_x) as u32) as usize] = true;
                    if let Some(color) = self.document.buffer.get(x as u32, y as u32) {
                        content.set((x - min_x) as u32, (y - min_y) as u32, color);
                    }
                }
            }
        }
        let boundary = rspaint_core::selection::boundary_segments(&box_mask, box_w, box_h);
        let runs = runs_from_mask(&box_mask, box_w, box_h);
        self.selection = Some(Selection {
            pixels: content,
            x: min_x,
            y: min_y,
            outline: Vec::new(),
            mask: true,
            boundary,
            mask_bits: box_mask,
            runs,
            floating: false,
            dragging: false,
            moved: false,
            drag_offset: (0, 0),
            base: None,
            name: "Selection Brush",
        });
    }

    fn selection_brush_stamp(&mut self, from: (i32, i32), to: (i32, i32)) {
        let (width, height) = (self.document.width(), self.document.height());
        let brush = Brush::new(self.brush_shape, self.brush_size);
        let subtract = self.sel_brush_subtract;
        let mask = &mut self.sel_brush_mask;
        let stamp = |cx: i32, cy: i32| {
            brush.visit(cx, cy, |px, py| {
                if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
                    mask[(py as u32 * width + px as u32) as usize] = !subtract;
                }
            });
        };
        if brush.size > 1 {
            walk_dense_line(from, to, stamp);
        } else {
            walk_line(from, to, stamp);
        }
        self.sel_brush_boundary =
            rspaint_core::selection::boundary_segments(&self.sel_brush_mask, width, height);
        self.sel_brush_runs = runs_from_mask(&self.sel_brush_mask, width, height);
    }

    /// The in-progress Selection Brush outline (canvas coordinates).
    pub fn selection_brush_boundary(&self) -> Option<&[rspaint_core::selection::BoundarySegment]> {
        self.sel_brush_active
            .then_some(self.sel_brush_boundary.as_slice())
    }

    /// The in-progress Selection Brush highlight runs (canvas coordinates).
    pub fn selection_brush_runs(&self) -> Option<&[HighlightRun]> {
        self.sel_brush_active
            .then_some(self.sel_brush_runs.as_slice())
    }

    /// The active selection's highlight runs and origin.
    pub fn selection_runs(&self) -> Option<(&[HighlightRun], (i32, i32))> {
        let selection = self.selection.as_ref()?;
        Some((selection.runs.as_slice(), (selection.x, selection.y)))
    }

    /// Selects the whole image (Ctrl+E).
    pub fn select_all(&mut self) {
        let width = self.document.width();
        let height = self.document.height();
        let pixels =
            rspaint_core::selection::extract_rect(&self.document.buffer, 0, 0, width, height);
        let region = NewRegion {
            pixels,
            x: 0,
            y: 0,
            mask_bits: vec![true; (width * height) as usize],
            outline: Vec::new(),
            mask: false,
            name: "Select All",
        };
        self.combine_new_region(region, false, false);
    }

    /// Whether a canvas point lies within the active selection.
    ///
    /// Rectangle and lasso selections use their bounding box; magic-wand
    /// selections use the actual pixel mask.
    pub fn point_in_selection(&self, p: (i32, i32)) -> bool {
        let Some(selection) = self.selection.as_ref() else {
            return false;
        };
        let lx = p.0 - selection.x;
        let ly = p.1 - selection.y;
        if lx < 0 || ly < 0 {
            return false;
        }
        let (lx, ly) = (lx as u32, ly as u32);
        if lx >= selection.pixels.width() || ly >= selection.pixels.height() {
            return false;
        }
        let index = (ly * selection.pixels.width() + lx) as usize;
        selection.mask_bits.get(index).copied().unwrap_or(true)
    }

    fn selection_fill(&self) -> Color {
        if self.document.transparent {
            Color::TRANSPARENT
        } else {
            self.background
        }
    }

    /// Begins a selection drag: moving an existing selection or creating a new
    /// one. Shift unions (`add`), Alt subtracts (`subtract`); both never move.
    pub fn begin_selection_drag(&mut self, p: (i32, i32), tool: Tool, add: bool, subtract: bool) {
        let combining = add || subtract;
        if !combining && self.point_in_selection(p) {
            self.start_moving_selection(p);
            return;
        }
        self.creating_selection_add = add;
        self.creating_selection_subtract = subtract;
        if !combining {
            self.deselect();
        }
        self.creating_selection = Some(match tool {
            Tool::FreeFormSelect => CreatingSelection::Lasso { points: vec![p] },
            _ => CreatingSelection::Rect { start: p, end: p },
        });
    }

    /// Updates the in-progress selection drag.
    pub fn update_selection_drag(&mut self, p: (i32, i32)) {
        if self.selection.as_ref().is_some_and(|s| s.dragging) {
            let offset = self
                .selection
                .as_ref()
                .map(|s| s.drag_offset)
                .unwrap_or((0, 0));
            // Snapshot once at the start of the move so Ctrl+Z can undo just
            // the move while keeping the selection.
            if self.selection.as_ref().is_some_and(|s| !s.moved) {
                // Snapshot the pre-move state: not floating, so it is
                // consistent with the unchanged canvas.
                let mut snapshot = self.selection.clone();
                if let Some(selection) = snapshot.as_mut() {
                    selection.dragging = false;
                    selection.floating = false;
                    selection.base = None;
                }
                self.selection_undo
                    .push((snapshot, self.document.buffer.clone()));
            }
            if let Some(selection) = self.selection.as_mut() {
                selection.moved = true;
            }
            self.move_selection_to(p.0 - offset.0, p.1 - offset.1);
            return;
        }
        match self.creating_selection.as_mut() {
            Some(CreatingSelection::Rect { end, .. }) => *end = p,
            Some(CreatingSelection::Lasso { points }) => {
                let far_enough = points
                    .last()
                    .is_none_or(|&last| (last.0 - p.0).abs() + (last.1 - p.1).abs() >= 1);
                if far_enough {
                    points.push(p);
                }
            }
            None => {}
        }
    }

    /// Ends the in-progress selection drag, finalizing a new selection.
    pub fn end_selection_drag(&mut self) {
        if let Some(selection) = self.selection.as_mut() {
            selection.dragging = false;
        }
        if self.creating_selection.is_some() {
            self.finish_creating_selection();
            // The Outliner applies as soon as the rectangle is drawn.
            if self.tool == Tool::Outliner && self.selection.is_some() {
                self.apply_outliner();
                self.deselect();
            }
        }
    }

    fn start_moving_selection(&mut self, p: (i32, i32)) {
        let fill = self.selection_fill();
        let background = if self.selection_transparent {
            Some(self.background)
        } else {
            None
        };
        let Some(selection) = self.selection.as_mut() else {
            return;
        };
        selection.dragging = true;
        selection.drag_offset = (p.0 - selection.x, p.1 - selection.y);
        if !selection.floating {
            let mut base = self.document.buffer.clone();
            rspaint_core::selection::clear_masked(
                &mut base,
                &selection.pixels,
                selection.x,
                selection.y,
                background,
                fill,
            );
            selection.base = Some(base);
            selection.floating = true;
        }
    }

    fn move_selection_to(&mut self, new_x: i32, new_y: i32) {
        let background = if self.selection_transparent {
            Some(self.background)
        } else {
            None
        };
        let mut dirty = None;
        if let Some(selection) = self.selection.as_mut() {
            let (old_x, old_y) = (selection.x, selection.y);
            let width = selection.pixels.width();
            let height = selection.pixels.height();
            selection.x = new_x;
            selection.y = new_y;

            let union = union_rect(old_x, old_y, new_x, new_y, width, height);
            if let Some((x, y, w, h)) = union.clamped(self.document.width(), self.document.height())
            {
                if let Some(base) = selection.base.as_ref() {
                    self.document.buffer.copy_region_from(base, x, y, w, h);
                }
            }
            rspaint_core::selection::stamp(
                &mut self.document.buffer,
                &selection.pixels,
                new_x,
                new_y,
                background,
            );
            dirty = Some(union);
        }
        if let Some(rect) = dirty {
            self.mark_rect(rect);
        }
    }

    fn finish_creating_selection(&mut self) {
        let Some(creating) = self.creating_selection.take() else {
            return;
        };
        let add = self.creating_selection_add;
        let subtract = self.creating_selection_subtract;
        self.creating_selection_add = false;
        self.creating_selection_subtract = false;

        let region: Option<NewRegion> = match creating {
            CreatingSelection::Rect { start, end } => {
                let x = start.0.min(end.0);
                let y = start.1.min(end.1);
                let width = (start.0.max(end.0) - x + 1) as u32;
                let height = (start.1.max(end.1) - y + 1) as u32;
                if width < 2 || height < 2 {
                    None
                } else {
                    let pixels = rspaint_core::selection::extract_rect(
                        &self.document.buffer,
                        x,
                        y,
                        width,
                        height,
                    );
                    if self.tool == Tool::ContourSelect {
                        // Select the pixels that differ from the secondary color.
                        let background = self.background;
                        let mut mask_bits = vec![false; (width * height) as usize];
                        let mut content = PixelBuffer::transparent(width, height);
                        for ly in 0..height {
                            for lx in 0..width {
                                if let Some(color) = pixels.get(lx, ly) {
                                    if color != background {
                                        mask_bits[(ly * width + lx) as usize] = true;
                                        content.set(lx, ly, color);
                                    }
                                }
                            }
                        }
                        let is_mask = !mask_bits.iter().all(|&selected| selected);
                        Some(NewRegion {
                            pixels: content,
                            x,
                            y,
                            mask_bits,
                            outline: Vec::new(),
                            mask: is_mask,
                            name: "Contour Select",
                        })
                    } else {
                        Some(NewRegion {
                            pixels,
                            x,
                            y,
                            mask_bits: vec![true; (width * height) as usize],
                            outline: Vec::new(),
                            mask: false,
                            name: "Select",
                        })
                    }
                }
            }
            CreatingSelection::Lasso { points } => {
                rspaint_core::selection::extract_lasso(&self.document.buffer, &points).map(
                    |(pixels, (x, y), mask_bits)| {
                        let outline = points.iter().map(|&(px, py)| (px - x, py - y)).collect();
                        NewRegion {
                            pixels,
                            x,
                            y,
                            mask_bits,
                            outline,
                            mask: false,
                            name: "Free-Form Select",
                        }
                    },
                )
            }
        };

        if let Some(region) = region {
            self.combine_new_region(region, add, subtract);
        }
    }

    /// Commits and clears the active selection.
    pub fn deselect(&mut self) {
        self.creating_selection = None;
        self.creating_selection_add = false;
        if let Some(selection) = self.selection.take() {
            if selection.floating && selection.moved {
                self.commit(selection.name);
            }
        }
        // Selection is finalized; its undo stack no longer applies.
        self.selection_undo.clear();
    }

    /// Copies the active selection (Ctrl+C) to the internal and system clipboards.
    pub fn copy_selection(&mut self) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        self.clipboard = Some(ClipboardContent {
            pixels: selection.pixels.clone(),
            mask_bits: selection.mask_bits.clone(),
            outline: selection.outline.clone(),
            mask: selection.mask,
        });
        crate::clipboard::write_image(&selection.pixels);
    }

    /// Pastes the clipboard as a floating selection (Ctrl+V).
    ///
    /// Prefers an image from the system clipboard; otherwise uses the last
    /// in-app copy. The pasted content stays a floating selection until the
    /// user clicks outside it.
    pub fn paste_clipboard(&mut self) {
        self.deselect();

        let content = match crate::clipboard::read_image() {
            Some(pixels) => {
                let count = (pixels.width() * pixels.height()) as usize;
                (pixels, vec![true; count], Vec::new(), false)
            }
            None => match self.clipboard.clone() {
                Some(content) => (
                    content.pixels,
                    content.mask_bits,
                    content.outline,
                    content.mask,
                ),
                None => return,
            },
        };
        let (pixels, mut mask_bits, outline, mask) = content;
        let width = pixels.width();
        let height = pixels.height();
        if width == 0 || height == 0 {
            return;
        }
        if mask_bits.len() != (width * height) as usize {
            mask_bits = vec![true; (width * height) as usize];
        }
        let boundary = if mask {
            rspaint_core::selection::boundary_segments(&mask_bits, width, height)
        } else {
            Vec::new()
        };
        let runs = runs_from_mask(&mask_bits, width, height);

        // Paste centered on the pointer (or the canvas center), as a floating
        // selection over the canvas.
        let (px, py) = self.pointer.unwrap_or((
            self.document.width() as i32 / 2,
            self.document.height() as i32 / 2,
        ));
        // Keep the whole pasted image inside the canvas.
        let max_x = (self.document.width() as i32 - width as i32).max(0);
        let max_y = (self.document.height() as i32 - height as i32).max(0);
        let x = (px - width as i32 / 2).clamp(0, max_x);
        let y = (py - height as i32 / 2).clamp(0, max_y);
        let background = if self.selection_transparent {
            Some(self.background)
        } else {
            None
        };
        let base = self.document.buffer.clone();
        rspaint_core::selection::stamp(&mut self.document.buffer, &pixels, x, y, background);

        self.set_tool(Tool::Select);
        self.selection = Some(Selection {
            pixels,
            x,
            y,
            outline,
            mask,
            boundary,
            mask_bits,
            runs,
            floating: true,
            dragging: false,
            moved: true,
            drag_offset: (0, 0),
            base: Some(base),
            name: "Paste",
        });
        self.mark_full();
    }

    /// Whether a canvas resize drag is in progress.
    pub fn resizing_canvas(&self) -> bool {
        self.resize_drag.is_some()
    }

    /// Begins a canvas resize drag from `handle`.
    pub fn begin_canvas_resize(
        &mut self,
        handle: ResizeHandle,
        start_min: egui::Pos2,
        magnification: f32,
    ) {
        self.resize_drag = Some(ResizeDrag {
            handle,
            orig_buffer: self.document.buffer.clone(),
            orig_width: self.document.width(),
            orig_height: self.document.height(),
            start_min,
            magnification,
            sel_orig: self.selection.as_ref().map(|s| (s.x, s.y)),
        });
    }

    /// Re-anchors the selection after the canvas changed size.
    ///
    /// The selection's `base`/floating state are reset because the resized
    /// buffer already contains the (possibly shifted) content.
    fn reanchor_selection_after_resize(&mut self, x: i32, y: i32) {
        if let Some(selection) = self.selection.as_mut() {
            selection.x = x;
            selection.y = y;
            selection.base = None;
            selection.floating = false;
            selection.moved = false;
            selection.dragging = false;
        }
        self.selection_undo.clear();
        self.selection_resize = None;
    }

    /// Updates the canvas size during a resize drag, anchored to the opposite edge.
    pub fn update_canvas_resize(&mut self, pointer: egui::Pos2, viewport_min: egui::Pos2) {
        let Some(drag) = self.resize_drag.take() else {
            return;
        };
        let mag = drag.magnification;
        let left = drag.start_min.x;
        let right = drag.start_min.x + drag.orig_width as f32 * mag;
        let top = drag.start_min.y;
        let bottom = drag.start_min.y + drag.orig_height as f32 * mag;

        let mut new_width = drag.orig_width;
        let mut new_height = drag.orig_height;
        let mut new_min_x = left;
        let mut new_min_y = top;

        if drag.handle.touches_east() {
            new_width = ((pointer.x - left) / mag).round().clamp(1.0, 100_000.0) as u32;
        } else if drag.handle.touches_west() {
            new_width = ((right - pointer.x) / mag).round().clamp(1.0, 100_000.0) as u32;
            new_min_x = right - new_width as f32 * mag;
        }
        if drag.handle.touches_south() {
            new_height = ((pointer.y - top) / mag).round().clamp(1.0, 100_000.0) as u32;
        } else if drag.handle.touches_north() {
            new_height = ((bottom - pointer.y) / mag).round().clamp(1.0, 100_000.0) as u32;
            new_min_y = bottom - new_height as f32 * mag;
        }

        let offset_x = if drag.handle.touches_west() {
            new_width as i32 - drag.orig_width as i32
        } else {
            0
        };
        let offset_y = if drag.handle.touches_north() {
            new_height as i32 - drag.orig_height as i32
        } else {
            0
        };

        let fill = self.background;
        self.document.buffer = drag
            .orig_buffer
            .resized_with_offset(new_width, new_height, offset_x, offset_y, fill);
        self.pan = egui::vec2(new_min_x - viewport_min.x, new_min_y - viewport_min.y);
        // Keep the selection anchored to its content after the resize.
        if let Some((origin_x, origin_y)) = drag.sel_orig {
            self.reanchor_selection_after_resize(origin_x + offset_x, origin_y + offset_y);
        }
        self.mark_full();
        self.resize_drag = Some(drag);
    }

    /// Finishes a canvas resize drag, recording it in history.
    pub fn finish_canvas_resize(&mut self) {
        if self.resize_drag.take().is_some() {
            self.commit("Resize Canvas");
        }
    }

    /// The selection's bounding box in image coordinates: `(x, y, width, height)`.
    pub fn selection_bounds(&self) -> Option<(i32, i32, u32, u32)> {
        let selection = self.selection.as_ref()?;
        Some((
            selection.x,
            selection.y,
            selection.pixels.width(),
            selection.pixels.height(),
        ))
    }

    /// Whether the selection is a plain rectangle (no free-form outline or mask).
    pub fn selection_is_simple_rect(&self) -> bool {
        self.selection.as_ref().is_some_and(|selection| {
            selection.outline.is_empty() && selection.mask_bits.iter().all(|&selected| selected)
        })
    }

    /// Whether a selection-content resize drag is in progress.
    pub fn resizing_selection(&self) -> bool {
        self.selection_resize.is_some()
    }

    /// Begins resizing the selection content from `handle`.
    pub fn begin_selection_resize(
        &mut self,
        handle: ResizeHandle,
        start_min: egui::Pos2,
        magnification: f32,
    ) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let background = if self.selection_transparent {
            Some(self.background)
        } else {
            None
        };
        let fill = self.selection_fill();
        let base = match selection.base.as_ref() {
            Some(base) => base.clone(),
            None => {
                let mut base = self.document.buffer.clone();
                rspaint_core::selection::clear_masked(
                    &mut base,
                    &selection.pixels,
                    selection.x,
                    selection.y,
                    background,
                    fill,
                );
                base
            }
        };
        // Ctrl+Z restores the selection and canvas before the resize.
        self.selection_undo
            .push((Some(selection.clone()), self.document.buffer.clone()));
        let base_for_selection = base.clone();
        self.selection_resize = Some(SelectionResizeDrag {
            handle,
            orig_pixels: selection.pixels.clone(),
            orig_mask: selection.mask_bits.clone(),
            orig_x: selection.x,
            orig_y: selection.y,
            orig_w: selection.pixels.width(),
            orig_h: selection.pixels.height(),
            base,
            start_min,
            magnification,
        });
        if let Some(selection) = self.selection.as_mut() {
            selection.floating = true;
            selection.base = Some(base_for_selection);
            selection.moved = true;
            selection.dragging = false;
            selection.name = "Resize Selection";
        }
        self.mark_full();
    }

    /// Updates the selection-content resize during a drag.
    pub fn update_selection_resize(&mut self, pointer: egui::Pos2) {
        let Some(drag) = self.selection_resize.take() else {
            return;
        };
        let mag = drag.magnification;
        let left = drag.start_min.x;
        let right = drag.start_min.x + drag.orig_w as f32 * mag;
        let top = drag.start_min.y;
        let bottom = drag.start_min.y + drag.orig_h as f32 * mag;

        let mut new_width = drag.orig_w;
        let mut new_height = drag.orig_h;
        let mut new_min_x = left;
        let mut new_min_y = top;
        if drag.handle.touches_east() {
            new_width = ((pointer.x - left) / mag).round().clamp(1.0, 100_000.0) as u32;
        } else if drag.handle.touches_west() {
            new_width = ((right - pointer.x) / mag).round().clamp(1.0, 100_000.0) as u32;
            new_min_x = right - new_width as f32 * mag;
        }
        if drag.handle.touches_south() {
            new_height = ((pointer.y - top) / mag).round().clamp(1.0, 100_000.0) as u32;
        } else if drag.handle.touches_north() {
            new_height = ((bottom - pointer.y) / mag).round().clamp(1.0, 100_000.0) as u32;
            new_min_y = bottom - new_height as f32 * mag;
        }

        let new_x = drag.orig_x + ((new_min_x - left) / mag).round() as i32;
        let new_y = drag.orig_y + ((new_min_y - top) / mag).round() as i32;

        let new_pixels = transform::scale(&drag.orig_pixels, new_width, new_height);
        let mask_source = mask_to_buffer(&drag.orig_mask, drag.orig_w, drag.orig_h);
        let new_mask = buffer_to_mask(&transform::scale(&mask_source, new_width, new_height));
        let is_rect = new_mask.iter().all(|&selected| selected);
        let boundary = if is_rect {
            Vec::new()
        } else {
            rspaint_core::selection::boundary_segments(&new_mask, new_width, new_height)
        };
        let runs = runs_from_mask(&new_mask, new_width, new_height);

        let background = if self.selection_transparent {
            Some(self.background)
        } else {
            None
        };
        let mut buffer = drag.base.clone();
        rspaint_core::selection::stamp(&mut buffer, &new_pixels, new_x, new_y, background);
        self.document.buffer = buffer;
        if let Some(selection) = self.selection.as_mut() {
            selection.pixels = new_pixels;
            selection.mask_bits = new_mask;
            selection.boundary = boundary;
            selection.runs = runs;
            selection.outline = Vec::new();
            selection.mask = !is_rect;
            selection.x = new_x;
            selection.y = new_y;
            selection.floating = true;
            selection.moved = true;
            selection.dragging = false;
            selection.base = Some(drag.base.clone());
            selection.name = "Resize Selection";
        }
        self.mark_full();
        self.selection_resize = Some(drag);
    }

    /// Finishes a selection-content resize (stays a floating selection).
    pub fn finish_selection_resize(&mut self) {
        self.selection_resize = None;
    }

    /// Deletes the selected pixels (filling with background or transparency).
    pub fn delete_selection(&mut self) {
        if self.selection.is_none() {
            return;
        }
        self.selection_undo
            .push((self.selection.clone(), self.document.buffer.clone()));
        let Some(selection) = self.selection.take() else {
            return;
        };
        self.creating_selection = None;
        let fill = self.selection_fill();
        let background = if self.selection_transparent {
            Some(self.background)
        } else {
            None
        };
        rspaint_core::selection::clear_masked(
            &mut self.document.buffer,
            &selection.pixels,
            selection.x,
            selection.y,
            background,
            fill,
        );
        self.mark_rect(RectI::between(
            (selection.x, selection.y),
            (
                selection.x + selection.pixels.width() as i32 - 1,
                selection.y + selection.pixels.height() as i32 - 1,
            ),
        ));
        self.commit("Delete Selection");
    }

    /// Opens an image file chosen with a native dialog.
    pub fn open_file_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Images",
                &["png", "bmp", "dib", "jpg", "jpeg", "gif", "tif", "tiff"],
            )
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.error_message = Some(format!("Failed to read file: {error}"));
                return;
            }
        };
        match rspaint_core::io::decode_image(&bytes) {
            Ok(buffer) => self.load_buffer(buffer, Some(path)),
            Err(error) => self.error_message = Some(format!("Failed to open image: {error}")),
        }
    }

    /// Replaces the document with a decoded image.
    pub fn load_buffer(&mut self, buffer: PixelBuffer, path: Option<std::path::PathBuf>) {
        let transparent = buffer.data().chunks_exact(4).any(|pixel| pixel[3] < 255);
        self.document.buffer = buffer;
        self.document.transparent = transparent;
        self.document.modified = false;
        self.document.name = path
            .as_deref()
            .and_then(file_stem)
            .unwrap_or_else(|| String::from("Untitled"));
        self.file_path = path;
        self.history = History::new(self.document.buffer.clone(), "Open");
        self.stroke = None;
        self.selection = None;
        self.creating_selection = None;
        self.selection_undo.clear();
        self.adjust_zoom();
        self.mark_full();
    }

    /// Fits the image height to the viewport.
    ///
    /// Centered when the scaled image also fits the viewport width; otherwise
    /// aligned to the top-left.
    pub fn adjust_zoom(&mut self) {
        let (width, height) = (self.document.width(), self.document.height());
        if self.viewport_size.y > 0.0 && height > 0 {
            let magnification =
                (self.viewport_size.y / height as f32).clamp(MIN_MAGNIFICATION, MAX_MAGNIFICATION);
            self.magnification = magnification;
        }
        let scaled = egui::vec2(
            width as f32 * self.magnification,
            height as f32 * self.magnification,
        );
        self.pan = if scaled.x <= self.viewport_size.x {
            (self.viewport_size - scaled) / 2.0
        } else {
            egui::Vec2::ZERO
        };
    }

    /// Records the canvas viewport size (used for the initial zoom on open).
    pub fn set_viewport_size(&mut self, size: egui::Vec2) {
        self.viewport_size = size;
        // On the first frame with a known viewport, fit the default sheet.
        if self.pending_adjust_zoom && size.y > 0.0 {
            self.adjust_zoom();
            self.pending_adjust_zoom = false;
        }
    }

    /// Saves to the current file, or asks for a location if there is none.
    pub fn save(&mut self) {
        match self.file_path.clone() {
            Some(path) => self.write_to(&path),
            None => self.save_as(),
        }
    }

    /// Saves to a location chosen with a native dialog.
    pub fn save_as(&mut self) {
        let default_name = format!("{}.png", self.document.name);
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .add_filter("BMP image", &["bmp", "dib"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.write_to(&path);
    }

    fn write_to(&mut self, path: &std::path::Path) {
        let format = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(rspaint_core::io::SaveFormat::from_extension)
            .unwrap_or(rspaint_core::io::SaveFormat::Png);
        match rspaint_core::io::encode(&self.document.buffer, format) {
            Ok(bytes) => match std::fs::write(path, bytes) {
                Ok(()) => {
                    self.file_path = Some(path.to_path_buf());
                    self.document.name =
                        file_stem(path).unwrap_or_else(|| String::from("Untitled"));
                    self.document.modified = false;
                }
                Err(error) => self.error_message = Some(format!("Failed to save: {error}")),
            },
            Err(error) => self.error_message = Some(format!("Failed to encode image: {error}")),
        }
    }

    /// Opens the Attributes dialog populated with the current document state.
    pub fn open_attributes_dialog(&mut self) {
        self.attributes_width = self.document.width();
        self.attributes_height = self.document.height();
        self.attributes_transparent = self.document.transparent;
        self.show_attributes = true;
    }

    /// Applies the Attributes dialog: resizes the canvas and sets transparency.
    pub fn apply_attributes(&mut self) {
        let width = self.attributes_width.max(1);
        let height = self.attributes_height.max(1);
        let fill = if self.attributes_transparent {
            Color::TRANSPARENT
        } else {
            self.background
        };
        if width != self.document.width() || height != self.document.height() {
            self.document.buffer = self.document.buffer.resized(width, height, fill);
        }
        if let Some((x, y)) = self.selection.as_ref().map(|s| (s.x, s.y)) {
            self.reanchor_selection_after_resize(x, y);
        }
        self.document.transparent = self.attributes_transparent;
        self.commit("Resize Canvas");
        self.mark_full();
        self.show_attributes = false;
    }

    /// Opens the Flip/Rotate dialog.
    pub fn open_flip_rotate_dialog(&mut self) {
        self.show_flip_rotate = true;
    }

    /// Applies the operation selected in the Flip/Rotate dialog.
    pub fn apply_flip_rotate(&mut self) {
        let choice = self.flip_rotate;
        let angle = self.flip_rotate_angle;
        let name = match choice {
            FlipRotate::FlipHorizontal => "Flip Horizontal",
            FlipRotate::FlipVertical => "Flip Vertical",
            FlipRotate::Rotate90 => "Rotate 90°",
            FlipRotate::Rotate180 => "Rotate 180°",
            FlipRotate::Rotate270 => "Rotate 270°",
            FlipRotate::Arbitrary => "Rotate",
        };
        let apply = |buffer: &PixelBuffer| match choice {
            FlipRotate::FlipHorizontal => transform::flip_horizontal(buffer),
            FlipRotate::FlipVertical => transform::flip_vertical(buffer),
            FlipRotate::Rotate90 => transform::rotate_90(buffer),
            FlipRotate::Rotate180 => transform::rotate_180(buffer),
            FlipRotate::Rotate270 => transform::rotate_270(buffer),
            FlipRotate::Arbitrary => transform::rotate(buffer, angle),
        };
        if self.selection.is_some() {
            self.transform_selection(apply, name);
        } else {
            self.document.buffer = apply(&self.document.buffer);
            self.pan = egui::Vec2::ZERO;
            self.mark_full();
            self.commit(name);
        }
        self.show_flip_rotate = false;
    }

    /// Applies a flip/rotate directly to the active selection (no-op if none).
    pub fn transform_selection_direct(&mut self, choice: FlipRotate) {
        if self.selection.is_none() {
            return;
        }
        let angle = self.flip_rotate_angle;
        let name = match choice {
            FlipRotate::FlipHorizontal => "Flip Horizontal",
            FlipRotate::FlipVertical => "Flip Vertical",
            FlipRotate::Rotate90 => "Rotate 90°",
            FlipRotate::Rotate180 => "Rotate 180°",
            FlipRotate::Rotate270 => "Rotate 270°",
            FlipRotate::Arbitrary => "Rotate",
        };
        let apply = |buffer: &PixelBuffer| match choice {
            FlipRotate::FlipHorizontal => transform::flip_horizontal(buffer),
            FlipRotate::FlipVertical => transform::flip_vertical(buffer),
            FlipRotate::Rotate90 => transform::rotate_90(buffer),
            FlipRotate::Rotate180 => transform::rotate_180(buffer),
            FlipRotate::Rotate270 => transform::rotate_270(buffer),
            FlipRotate::Arbitrary => transform::rotate(buffer, angle),
        };
        self.transform_selection(apply, name);
    }

    /// Replaces the foreground color with the background color.
    ///
    /// If there is a selection, only the pixels inside it are affected;
    /// otherwise the whole image is.
    pub fn replace_primary_with_secondary(&mut self) {
        let from = self.foreground;
        let to = self.background;
        if from == to {
            return;
        }

        if let Some(selection) = self.selection.as_ref() {
            let (ox, oy) = (selection.x, selection.y);
            let (w, h) = (selection.pixels.width(), selection.pixels.height());
            // Update the canvas within the selection...
            for ly in 0..h {
                for lx in 0..w {
                    if !selection
                        .mask_bits
                        .get((ly * w + lx) as usize)
                        .copied()
                        .unwrap_or(true)
                    {
                        continue;
                    }
                    let cx = ox + lx as i32;
                    let cy = oy + ly as i32;
                    if cx >= 0
                        && cy >= 0
                        && self.document.buffer.get(cx as u32, cy as u32) == Some(from)
                    {
                        self.document.buffer.set(cx as u32, cy as u32, to);
                    }
                }
            }
            // ...and the selection's floating content.
            if let Some(selection) = self.selection.as_mut() {
                for ly in 0..h {
                    for lx in 0..w {
                        if selection
                            .mask_bits
                            .get((ly * w + lx) as usize)
                            .copied()
                            .unwrap_or(true)
                            && selection.pixels.get(lx, ly) == Some(from)
                        {
                            selection.pixels.set(lx, ly, to);
                        }
                    }
                }
            }
        } else {
            rspaint_core::fill::replace_color(&mut self.document.buffer, from, to);
        }

        self.mark_full();
        self.commit("Replace Color");
    }

    /// Applies the Outliner tool to the current (rectangular) selection.
    ///
    /// Detects the contour of the non-background colors inside the selection
    /// and paints a border with the foreground color (and the extra color at
    /// concave corners), scoped to the selection's bounding box.
    pub fn apply_outliner(&mut self) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let rx = selection.x;
        let ry = selection.y;
        let rw = selection.pixels.width() as i32;
        let rh = selection.pixels.height() as i32;
        if rw < 1 || rh < 1 {
            return;
        }

        let bg = self.background;
        let fg = self.foreground;
        let extra = self.outliner_color;
        let size = self.outliner_size.max(1) as i32;
        let concave = self.outliner_concave;
        let (outward, inward) = match self.outliner_placement {
            BorderPlacement::Outside => (size, 0),
            BorderPlacement::OneInward => (size.saturating_sub(1), 1),
            BorderPlacement::Symmetric => ((size + 1) / 2, size / 2),
        };

        let region = rspaint_core::selection::extract_rect(
            &self.document.buffer,
            rx,
            ry,
            rw as u32,
            rh as u32,
        );
        let in_rect = |lx: i32, ly: i32| lx >= 0 && ly >= 0 && lx < rw && ly < rh;
        let is_bg =
            |lx: i32, ly: i32| in_rect(lx, ly) && region.get(lx as u32, ly as u32) == Some(bg);
        let is_obj =
            |lx: i32, ly: i32| in_rect(lx, ly) && region.get(lx as u32, ly as u32) != Some(bg);
        let area = (rw * rh) as usize;
        let index = |lx: i32, ly: i32| (ly * rw + lx) as usize;
        let orth = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)];

        // Outside region: 4-connected flood fill of the background color from
        // the rectangle's edges.
        let mut outside = vec![false; area];
        let mut stack: Vec<(i32, i32)> = Vec::new();
        for lx in 0..rw {
            for ly in [0, rh - 1] {
                if is_bg(lx, ly) {
                    stack.push((lx, ly));
                }
            }
        }
        for ly in 0..rh {
            for lx in [0, rw - 1] {
                if is_bg(lx, ly) {
                    stack.push((lx, ly));
                }
            }
        }
        while let Some((lx, ly)) = stack.pop() {
            if !is_bg(lx, ly) {
                continue;
            }
            let i = index(lx, ly);
            if outside[i] {
                continue;
            }
            outside[i] = true;
            for (dx, dy) in orth {
                stack.push((lx + dx, ly + dy));
            }
        }

        let mut painted: Vec<Option<Color>> = vec![None; area];

        // Outward layers: background pixels, starting from the contour.
        let mut prev: Vec<bool> = vec![false; area];
        for layer in 0..outward {
            let mut current = vec![false; area];
            for ly in 0..rh {
                for lx in 0..rw {
                    let i = index(lx, ly);
                    if painted[i].is_some() || !is_bg(lx, ly) {
                        continue;
                    }
                    let adjacent = if layer == 0 {
                        outside[i] && orth.iter().any(|&(dx, dy)| is_obj(lx + dx, ly + dy))
                    } else {
                        orth.iter().any(|&(dx, dy)| {
                            in_rect(lx + dx, ly + dy) && prev[index(lx + dx, ly + dy)]
                        })
                    };
                    if adjacent {
                        painted[i] = Some(fg);
                        current[i] = true;
                    }
                }
            }
            prev = current;
        }

        // Inward layers: object pixels, starting from the contour's object side.
        let mut prev: Vec<bool> = vec![false; area];
        for layer in 0..inward {
            let mut current = vec![false; area];
            for ly in 0..rh {
                for lx in 0..rw {
                    let i = index(lx, ly);
                    if painted[i].is_some() || !is_obj(lx, ly) {
                        continue;
                    }
                    let adjacent = if layer == 0 {
                        orth.iter().any(|&(dx, dy)| {
                            is_bg(lx + dx, ly + dy)
                                && in_rect(lx + dx, ly + dy)
                                && outside[index(lx + dx, ly + dy)]
                        })
                    } else {
                        orth.iter().any(|&(dx, dy)| {
                            in_rect(lx + dx, ly + dy) && prev[index(lx + dx, ly + dy)]
                        })
                    };
                    if adjacent {
                        painted[i] = Some(fg);
                        current[i] = true;
                    }
                }
            }
            prev = current;
        }

        // Concave corners pass (after the last layer): background pixels
        // adjacent to the painted border that sit in a concave corner get the
        // extra color.
        if concave {
            // Snapshot of the border only, so the extra-color pixels painted
            // here don't count as border and don't cascade.
            let border: Vec<bool> = painted.iter().map(|p| p.is_some()).collect();
            for ly in 0..rh {
                for lx in 0..rw {
                    let i = index(lx, ly);
                    // A background pixel on the outside of the border that has
                    // two orthogonal neighbors of the border color (a concave
                    // corner) gets the extra color.
                    if !is_bg(lx, ly) || painted[i].is_some() || !outside[i] {
                        continue;
                    }
                    let up = in_rect(lx, ly - 1) && border[index(lx, ly - 1)];
                    let down = in_rect(lx, ly + 1) && border[index(lx, ly + 1)];
                    let left = in_rect(lx - 1, ly) && border[index(lx - 1, ly)];
                    let right = in_rect(lx + 1, ly) && border[index(lx + 1, ly)];
                    if (up || down) && (left || right) {
                        painted[i] = Some(extra);
                    }
                }
            }
        }

        let mut changed = false;
        for ly in 0..rh {
            for lx in 0..rw {
                if let Some(color) = painted[index(lx, ly)] {
                    let x = rx + lx;
                    let y = ry + ly;
                    if x >= 0
                        && y >= 0
                        && self.document.buffer.get(x as u32, y as u32) != Some(color)
                    {
                        self.document.buffer.set(x as u32, y as u32, color);
                        changed = true;
                    }
                }
            }
        }

        if changed {
            self.mark_full();
            self.commit("Outliner");
        }
    }

    /// Inverts the colors of the whole image.
    pub fn invert_colors(&mut self) {
        let inverted = transform::invert(&self.document.buffer);
        self.document.buffer = inverted;
        self.mark_full();
        self.commit("Invert Colors");
    }

    /// Opens the Stretch/Skew dialog.
    pub fn open_stretch_skew_dialog(&mut self) {
        self.show_stretch_skew = true;
    }

    /// Applies the Stretch/Skew dialog values to the whole image.
    pub fn apply_stretch_skew(&mut self) {
        let x_scale = (self.stretch_x / 100.0).max(0.01);
        let y_scale = (self.stretch_y / 100.0).max(0.01);
        let h_skew = self.skew_h.to_radians();
        let v_skew = self.skew_v.to_radians();
        let fill = if self.document.transparent {
            Color::TRANSPARENT
        } else {
            self.background
        };
        let stretched = self.stretch_x != 100.0 || self.stretch_y != 100.0;
        let skewed = self.skew_h != 0.0 || self.skew_v != 0.0;
        let name = match (stretched, skewed) {
            (true, true) => "Stretch and Skew",
            (true, false) => "Stretch",
            (false, true) => "Skew",
            (false, false) => "Stretch and Skew",
        };
        let apply = |buffer: &PixelBuffer| {
            transform::stretch_and_skew(buffer, x_scale, y_scale, h_skew, v_skew, fill)
        };
        if self.selection.is_some() {
            self.transform_selection(apply, name);
        } else {
            self.document.buffer = apply(&self.document.buffer);
            self.pan = egui::Vec2::ZERO;
            self.mark_full();
            self.commit(name);
        }
        self.show_stretch_skew = false;
    }

    /// Applies a pixel transform to the active selection, keeping its position.
    ///
    /// The selection's mask is transformed alongside its pixels so combined
    /// (multi-region) selections transform as a whole around their bounding box.
    fn transform_selection<F>(&mut self, apply: F, name: &'static str)
    where
        F: Fn(&PixelBuffer) -> PixelBuffer,
    {
        let Some(selection) = self.selection.take() else {
            return;
        };
        // Ctrl+Z restores the selection and canvas before the transform.
        self.selection_undo
            .push((Some(selection.clone()), self.document.buffer.clone()));
        let background = if self.selection_transparent {
            Some(self.background)
        } else {
            None
        };
        let fill = self.selection_fill();

        // Canvas with the selection region cleared, so the transformed content
        // can be stamped fresh (reusing the existing base when already floating).
        let base = match selection.base.as_ref() {
            Some(base) => base.clone(),
            None => {
                let mut base = self.document.buffer.clone();
                rspaint_core::selection::clear_masked(
                    &mut base,
                    &selection.pixels,
                    selection.x,
                    selection.y,
                    background,
                    fill,
                );
                base
            }
        };

        let new_pixels = apply(&selection.pixels);
        let mask_source = mask_to_buffer(
            &selection.mask_bits,
            selection.pixels.width(),
            selection.pixels.height(),
        );
        let new_mask_bits = buffer_to_mask(&apply(&mask_source));
        let width = new_pixels.width();
        let height = new_pixels.height();
        let is_rect = new_mask_bits.iter().all(|&selected| selected);
        let boundary = if is_rect {
            Vec::new()
        } else {
            rspaint_core::selection::boundary_segments(&new_mask_bits, width, height)
        };
        let runs = runs_from_mask(&new_mask_bits, width, height);

        let (x, y) = (selection.x, selection.y);
        let mut buffer = base.clone();
        rspaint_core::selection::stamp(&mut buffer, &new_pixels, x, y, background);
        self.document.buffer = buffer;
        self.selection = Some(Selection {
            pixels: new_pixels,
            x,
            y,
            outline: Vec::new(),
            mask: !is_rect,
            boundary,
            mask_bits: new_mask_bits,
            runs,
            floating: true,
            dragging: false,
            moved: true,
            drag_offset: (0, 0),
            base: Some(base),
            name,
        });
        // The transform stays a floating selection (like a move); it is
        // committed to history when the selection is deselected.
        self.mark_full();
    }

    /// Crops the canvas to the active selection's bounds.
    pub fn crop_to_selection(&mut self) {
        let Some((x, y, width, height)) = self
            .selection
            .as_ref()
            .map(|s| (s.x, s.y, s.pixels.width(), s.pixels.height()))
        else {
            return;
        };
        self.selection = None;
        self.creating_selection = None;
        let cropped = transform::crop(&self.document.buffer, x, y, width, height);
        self.document.buffer = cropped;
        self.pan = egui::Vec2::ZERO;
        self.mark_full();
        self.commit("Crop");
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let marker = if self.document.modified { " *" } else { "" };
        let title = format!("{}{} - RS Paint", self.document.name, marker);
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn mark_full(&mut self) {
        self.canvas_dirty = true;
        self.dirty_rect = None;
    }

    fn mark_rect(&mut self, rect: RectI) {
        self.dirty_rect = Some(match self.dirty_rect {
            Some(existing) => existing.union(rect),
            None => rect,
        });
    }

    /// Marks the bounding box of a line segment expanded by a radius.
    fn mark_segment(&mut self, a: (i32, i32), b: (i32, i32), radius: i32) {
        self.mark_rect(RectI::between(a, b).expand(radius));
    }

    fn tool_radius(&self) -> i32 {
        match self.tool {
            Tool::Pencil => (self.pencil_size / 2) as i32,
            Tool::Brush => (self.brush_size / 2) as i32 + 1,
            Tool::Eraser => (self.eraser_size / 2) as i32 + 1,
            Tool::Airbrush => self.airbrush_size as i32 + 1,
            _ => self.stroke_size as i32 + 1,
        }
    }

    fn active_color(&self, button: PointerButton) -> Color {
        match button {
            PointerButton::Secondary => self.background,
            _ => self.foreground,
        }
    }

    fn canvas_clear_color(&self) -> Color {
        if self.document.transparent {
            Color::TRANSPARENT
        } else {
            self.background
        }
    }

    fn eraser_color(&self) -> Color {
        if self.document.transparent {
            Color::TRANSPARENT
        } else {
            self.background
        }
    }

    /// Starts a stroke at `p` using the given mouse button.
    pub fn begin_stroke(&mut self, p: (i32, i32), button: PointerButton) {
        let color = self.active_color(button);
        self.note_used_color(color);
        // The fill color is the opposite of the stroke color (right-drag swaps).
        let fill_color = if button == PointerButton::Secondary {
            self.foreground
        } else {
            self.background
        };
        let tool = self.tool;
        if tool.is_shape() {
            let preview_rect = RectI::between(p, p).expand(self.stroke_size as i32 + 1);
            self.stroke = Some(Stroke {
                color,
                fill_color,
                start: p,
                last: p,
                name: tool.name(),
                color_eraser: false,
                base: Some(self.document.buffer.clone()),
                preview_rect: Some(preview_rect),
            });
            self.draw_shape(p, p, color, fill_color);
            self.mark_rect(preview_rect);
        } else if tool.is_freehand() {
            let color_eraser = tool == Tool::Eraser && button == PointerButton::Secondary;
            self.stroke = Some(Stroke {
                color,
                fill_color,
                start: p,
                last: p,
                name: if color_eraser {
                    "Color Eraser"
                } else {
                    tool.name()
                },
                color_eraser,
                base: None,
                preview_rect: None,
            });
            self.draw_freehand(p, p, color);
            self.mark_segment(p, p, self.tool_radius());
        }
    }

    /// Continues the active stroke to `p`.
    ///
    /// When `shift` is held, box shapes (Rectangle, Ellipse, Rounded Rectangle)
    /// are constrained to a square/circle sized to the larger cursor delta.
    pub fn update_stroke(&mut self, p: (i32, i32), shift: bool) {
        let Some((last, start, color)) = self.stroke.as_ref().map(|s| (s.last, s.start, s.color))
        else {
            return;
        };
        if self.tool.is_shape() {
            let end = self.shape_end(start, p, shift);
            self.redraw_shape(end);
        } else {
            self.draw_freehand(last, p, color);
            self.mark_segment(last, p, self.tool_radius());
        }
        if let Some(stroke) = self.stroke.as_mut() {
            stroke.last = p;
        }
    }

    /// Finishes the active stroke and records it in history.
    pub fn end_stroke(&mut self, shift: bool) {
        // Redraw the final shape in case Shift was toggled after the last move.
        if self.tool.is_shape() {
            if let Some((start, last)) = self.stroke.as_ref().map(|s| (s.start, s.last)) {
                let end = self.shape_end(start, last, shift);
                self.redraw_shape(end);
            }
        }
        if let Some(stroke) = self.stroke.take() {
            self.commit(stroke.name);
        }
    }

    /// The effective end point of a shape, applying the Shift constraint.
    ///
    /// Box shapes (Rectangle, Ellipse, Rounded Rectangle) become a square
    /// sized to the larger cursor delta; the Line snaps to eight directions
    /// (multiples of 45°) while keeping the cursor distance.
    fn shape_end(&self, start: (i32, i32), p: (i32, i32), shift: bool) -> (i32, i32) {
        if !shift {
            return p;
        }
        match self.tool {
            Tool::Line => snap_to_45(start, p),
            Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle => square_end(start, p),
            _ => p,
        }
    }

    /// Restores the previous shape preview and draws the shape from the stroke
    /// start to `end`.
    fn redraw_shape(&mut self, end: (i32, i32)) {
        let Some((start, color, fill)) = self
            .stroke
            .as_ref()
            .map(|s| (s.start, s.color, s.fill_color))
        else {
            return;
        };
        let new_rect = RectI::between(start, end).expand(self.stroke_size as i32 + 1);
        let previous = self.stroke.as_ref().and_then(|s| s.preview_rect);
        let restore = match previous {
            Some(previous) => previous.union(new_rect),
            None => new_rect,
        };
        if let Some((x, y, w, h)) = restore.clamped(self.document.width(), self.document.height()) {
            if let Some(base) = self.stroke.as_ref().and_then(|s| s.base.as_ref()) {
                self.document.buffer.copy_region_from(base, x, y, w, h);
            }
        }
        self.draw_shape(start, end, color, fill);
        if let Some(stroke) = self.stroke.as_mut() {
            stroke.preview_rect = Some(new_rect);
        }
        self.mark_rect(restore);
    }

    fn draw_freehand(&mut self, from: (i32, i32), to: (i32, i32), color: Color) {
        match self.tool {
            Tool::Pencil => {
                let brush = Brush::new(BrushShape::Square, self.pencil_size);
                draw_line(&mut self.document.buffer, from, to, &brush, color);
            }
            Tool::Brush => {
                let brush = Brush::new(self.brush_shape, self.brush_size);
                draw_line(&mut self.document.buffer, from, to, &brush, color);
            }
            Tool::Eraser => {
                let brush = Brush::new(self.eraser_shape, self.eraser_size);
                if self
                    .stroke
                    .as_ref()
                    .is_some_and(|stroke| stroke.color_eraser)
                {
                    let erase_to = self.eraser_color();
                    let erase_from = self.foreground;
                    erase_line(
                        &mut self.document.buffer,
                        from,
                        to,
                        &brush,
                        erase_from,
                        erase_to,
                    );
                } else {
                    let color = self.eraser_color();
                    draw_line(&mut self.document.buffer, from, to, &brush, color);
                }
            }
            Tool::Airbrush => self.spray(to, color),
            _ => {}
        }
    }

    fn spray(&mut self, at: (i32, i32), color: Color) {
        let radius = self.airbrush_size.max(1) as i32;
        let density = (radius * radius) as u32;
        for _ in 0..density {
            let span = (2 * radius + 1) as u32;
            let dx = (self.rng.next_u32() % span) as i32 - radius;
            let dy = (self.rng.next_u32() % span) as i32 - radius;
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let x = at.0 + dx;
            let y = at.1 + dy;
            if x >= 0 && y >= 0 {
                self.document.buffer.set(x as u32, y as u32, color);
            }
        }
    }

    fn draw_shape(&mut self, a: (i32, i32), b: (i32, i32), color: Color, fill: Color) {
        let brush = Brush::new(BrushShape::Square, self.stroke_size);
        let fill_style = self.fill_style;
        // Whether to fill the interior, and with which color.
        let interior = match fill_style {
            ShapeFill::Outline => None,
            ShapeFill::FilledWithOutline => Some(fill),
            ShapeFill::FilledOnly => Some(color),
        };
        let outline = fill_style != ShapeFill::FilledOnly;
        match self.tool {
            Tool::Line => draw_line(&mut self.document.buffer, a, b, &brush, color),
            Tool::Rectangle => {
                if let Some(interior) = interior {
                    fill_rectangle(&mut self.document.buffer, a, b, interior);
                }
                if outline {
                    draw_rectangle(&mut self.document.buffer, a, b, &brush, color);
                }
            }
            Tool::Ellipse => {
                if let Some(interior) = interior {
                    fill_ellipse(&mut self.document.buffer, a, b, interior);
                }
                if outline {
                    draw_ellipse(&mut self.document.buffer, a, b, &brush, color);
                }
            }
            Tool::RoundedRectangle => {
                let radius = self.round_radius as i32;
                if let Some(interior) = interior {
                    fill_rounded_rectangle(&mut self.document.buffer, a, b, radius, interior);
                }
                if outline {
                    draw_rounded_rectangle(&mut self.document.buffer, a, b, radius, &brush, color);
                }
            }
            _ => {}
        }
    }

    /// Updates the temporary color-pick state from the modifier keys.
    ///
    /// While a drawing tool is active, Ctrl picks the primary color and Alt
    /// picks the secondary; releasing restores the tool.
    fn update_pick_hold(&mut self, ctx: &egui::Context) {
        let (ctrl, alt) = ctx.input(|i| (i.modifiers.command, i.modifiers.alt));
        self.pick_hold = if !self.tool.allows_color_pick() {
            None
        } else if alt {
            Some(PickTarget::Secondary)
        } else if ctrl {
            Some(PickTarget::Primary)
        } else {
            None
        };
    }

    /// The active temporary color-pick, if any.
    pub fn pick_hold(&self) -> Option<PickTarget> {
        self.pick_hold
    }

    /// Draws the custom tool cursor with its hotspot at `pointer` (screen space).
    pub fn paint_cursor(&self, painter: &egui::Painter, pointer: egui::Pos2) {
        self.cursors
            .for_state(self.tool, self.pick_hold)
            .paint(painter, pointer);
    }

    /// Picks the color under `p` into the given active color.
    pub fn pick_color_at(&mut self, p: (i32, i32), target: PickTarget) {
        if p.0 < 0 || p.1 < 0 {
            return;
        }
        if let Some(color) = self.document.buffer.get(p.0 as u32, p.1 as u32) {
            match target {
                PickTarget::Primary => self.foreground = color,
                PickTarget::Secondary => self.background = color,
            }
        }
    }

    /// Handles a click for click-based tools (Fill, Pick Color, Magnifier).
    pub fn click_tool(&mut self, p: (i32, i32), button: PointerButton, shift: bool) {
        match self.tool {
            Tool::Fill => {
                let color = self.active_color(button);
                self.note_used_color(color);
                flood_fill(&mut self.document.buffer, p.0, p.1, color, !shift);
                self.commit("Fill With Color");
                self.mark_full();
            }
            Tool::PickColor => {
                if p.0 >= 0 && p.1 >= 0 {
                    if let Some(color) = self.document.buffer.get(p.0 as u32, p.1 as u32) {
                        if button == PointerButton::Secondary {
                            self.background = color;
                        } else {
                            self.foreground = color;
                        }
                    }
                }
            }
            Tool::Magnifier => {
                if button == PointerButton::Secondary {
                    self.zoom_out();
                } else {
                    self.zoom_in();
                }
            }
            _ => {}
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        let mut select_tool = None;
        let mut action = None;
        let mut ctrl_tap = false;
        let mut size_delta = 0i32;
        let mut radius_delta = 0i32;
        ctx.input_mut(|i| {
            // While editing a text box, the editor consumes the keyboard; only
            // Escape (commit) is handled here.
            if self.has_text_box() {
                self.ctrl_down = i.modifiers.command;
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.commit_text();
                }
                return;
            }
            if i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals) {
                size_delta += 1;
            }
            if i.key_pressed(Key::Minus) {
                size_delta -= 1;
            }
            // `/` and `*` adjust the Rounded Rectangle corner radius. Text
            // events capture the actual character, so they work on any layout
            // and for the numpad `*` (which has no egui key variant).
            for event in &i.events {
                if let egui::Event::Text(text) = event {
                    match text.as_str() {
                        "/" => radius_delta -= 1,
                        "*" => radius_delta += 1,
                        _ => {}
                    }
                }
            }
            // Detect a bare command-key (Ctrl/Cmd) tap: pressed and released
            // without being combined with a key, wheel or zoom.
            let command = i.modifiers.command;
            let combined = i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { pressed: true, .. }
                        | egui::Event::PointerButton { pressed: true, .. }
                        | egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::Paste(_)
                        | egui::Event::MouseWheel { .. }
                        | egui::Event::Zoom(_)
                )
            });
            // egui-winit swallows the Ctrl+C/V press (emitting
            // `Event::Copy`/`Event::Paste` instead), so copy/paste are detected
            // on key release, which is always delivered.
            let copy_cmd = i
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy));
            let paste_cmd = i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { key: Key::V, pressed: false, modifiers, .. }
                        if modifiers.command
                )
            });
            if command && !self.ctrl_down {
                self.ctrl_tap_used = false;
            }
            if command && (combined || paste_cmd) {
                self.ctrl_tap_used = true;
            }
            if !command && self.ctrl_down && !self.ctrl_tap_used {
                ctrl_tap = true;
            }
            self.ctrl_down = command;

            if i.consume_key(ctrl_shift, Key::Z) || i.consume_key(Modifiers::CTRL, Key::Y) {
                self.redo();
            } else if i.consume_key(Modifiers::CTRL, Key::Z) {
                self.undo();
            } else if i.consume_key(ctrl_shift, Key::S) {
                action = Some(FileAction::SaveAs);
            } else if i.consume_key(Modifiers::CTRL, Key::S) {
                action = Some(FileAction::Save);
            } else if i.consume_key(Modifiers::CTRL, Key::O) {
                action = Some(FileAction::Open);
            } else if i.consume_key(ctrl_shift, Key::Y) {
                self.show_history = true;
            } else if i.consume_key(Modifiers::CTRL, Key::I) {
                self.invert_colors();
            } else if i.consume_key(Modifiers::CTRL, Key::E) {
                self.select_all();
                // With the Outliner, Ctrl+E applies it to the whole image.
                if self.tool == Tool::Outliner {
                    self.apply_outliner();
                    self.deselect();
                }
            } else if i.consume_key(Modifiers::CTRL, Key::R) {
                self.show_flip_rotate = true;
            } else if i.consume_key(Modifiers::CTRL, Key::W) {
                self.show_stretch_skew = true;
            } else if copy_cmd {
                self.copy_selection();
            } else if paste_cmd {
                self.paste_clipboard();
            } else if i.consume_key(Modifiers::NONE, Key::Delete)
                || i.consume_key(Modifiers::NONE, Key::Backspace)
            {
                self.delete_selection();
            } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                if self.has_text_box() {
                    self.commit_text();
                } else if !self.curve_points.is_empty() {
                    self.curve_cancel();
                } else if self.polygon_points.is_empty() {
                    self.deselect();
                } else {
                    self.cancel_polygon();
                }
            }
            for tool in Tool::ALL {
                if let Some(c) = tool.shortcut() {
                    if let Some(key) = key_for_char(c) {
                        if i.consume_key(Modifiers::NONE, key) {
                            select_tool = Some(tool);
                        }
                    }
                }
            }
        });
        if let Some(tool) = select_tool {
            if tool.implemented() {
                self.set_tool(tool);
            }
        }
        match action {
            Some(FileAction::Save) => self.save(),
            Some(FileAction::SaveAs) => self.save_as(),
            Some(FileAction::Open) => self.open_file_dialog(),
            None => {}
        }
        if ctrl_tap {
            self.toggle_selection_transparency();
        }
        if size_delta != 0 {
            self.adjust_tool_size(size_delta);
        }
        if radius_delta != 0 && self.tool == Tool::RoundedRectangle {
            self.adjust_round_radius(radius_delta);
        }
    }

    /// Adjusts the Rounded Rectangle corner radius (the `/` and `*` keys).
    pub fn adjust_round_radius(&mut self, delta: i32) {
        let next = (self.round_radius as i32 + delta).clamp(0, MAX_ROUND_RADIUS as i32);
        self.round_radius = next as u32;
    }

    /// Increases or decreases the active tool's size (the `+`/`-` keys).
    pub fn adjust_tool_size(&mut self, delta: i32) {
        match self.tool {
            Tool::Pencil => step_size(&mut self.pencil_size, delta, 50),
            Tool::Brush | Tool::SelectionBrush => step_size(&mut self.brush_size, delta, 500),
            Tool::Eraser => step_size(&mut self.eraser_size, delta, 500),
            Tool::Airbrush => step_size(&mut self.airbrush_size, delta, 500),
            Tool::Line | Tool::Curve | Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle => {
                step_size(&mut self.stroke_size, delta, 500);
            }
            _ => {}
        }
    }

    /// Toggles whether the active selection is transparent, re-rendering it.
    pub fn toggle_selection_transparency(&mut self) {
        if self.selection.is_none() {
            return;
        }
        self.selection_transparent = !self.selection_transparent;
        self.restamp_selection();
    }

    /// Re-composites a floating selection after its transparency mode changed.
    fn restamp_selection(&mut self) {
        let background = if self.selection_transparent {
            Some(self.background)
        } else {
            None
        };
        let mut dirty = None;
        if let Some(selection) = self.selection.as_mut() {
            if selection.floating {
                let (x, y) = (selection.x, selection.y);
                let rect = RectI::between(
                    (x, y),
                    (
                        x + selection.pixels.width() as i32 - 1,
                        y + selection.pixels.height() as i32 - 1,
                    ),
                );
                if let Some((cx, cy, cw, ch)) =
                    rect.clamped(self.document.width(), self.document.height())
                {
                    if let Some(base) = selection.base.as_ref() {
                        self.document.buffer.copy_region_from(base, cx, cy, cw, ch);
                    }
                }
                rspaint_core::selection::stamp(
                    &mut self.document.buffer,
                    &selection.pixels,
                    x,
                    y,
                    background,
                );
                dirty = Some(rect);
            }
        }
        if let Some(rect) = dirty {
            self.mark_rect(rect);
        }
    }

    /// Builds the uniform data for the canvas shader.
    ///
    /// `uv_rect` is the visible sub-rectangle of the canvas as
    /// `[min_x, min_y, size_x, size_y]` in `0..1` uv space.
    pub fn canvas_uniforms(
        &self,
        magnification: f32,
        pixels_per_point: f32,
        uv_rect: [f32; 4],
    ) -> Uniforms {
        let magnification_px = magnification * pixels_per_point;
        let (cursor, show_cursor) = match self.pointer {
            Some((cx, cy))
                if cx >= 0
                    && cy >= 0
                    && cx < self.document.width() as i32
                    && cy < self.document.height() as i32 =>
            {
                let size = self.cursor_size().max(1);
                let offset = (size - 1) / 2;
                let origin_x = (cx - offset as i32) as f32;
                let origin_y = (cy - offset as i32) as f32;
                let shape = self.cursor_shape_code();
                ([origin_x, origin_y, size as f32, shape], true)
            }
            _ => ([0.0; 4], false),
        };
        Uniforms {
            canvas_mag: [
                self.document.width() as f32,
                self.document.height() as f32,
                magnification_px,
                8.0 * pixels_per_point,
            ],
            cursor,
            flags: [
                bool_f32(self.document.transparent),
                bool_f32(magnification >= 8.0),
                bool_f32(show_cursor),
                bool_f32(self.srgb_output),
            ],
            uv_rect,
            params: [self.grid_opacity as f32 / 100.0, 0.0, 0.0, 0.0],
        }
    }

    /// Takes the pending canvas upload (full or partial) for the renderer.
    pub fn take_pending_upload(&mut self) -> Option<Upload> {
        let width = self.document.width();
        let height = self.document.height();

        if self.canvas_dirty {
            self.canvas_dirty = false;
            self.dirty_rect = None;
            return Some(Upload {
                x: 0,
                y: 0,
                width,
                height,
                data: self.document.buffer.data().to_vec(),
            });
        }

        let rect = self.dirty_rect.take()?;
        let (x, y, w, h) = if rect.covers(width, height) {
            (0, 0, width, height)
        } else {
            rect.clamped(width, height)?
        };
        Some(Upload {
            x,
            y,
            width: w,
            height: h,
            data: self.document.buffer.region_rgba(x, y, w, h),
        })
    }
}

fn bool_f32(value: bool) -> f32 {
    if value {
        1.0
    } else {
        0.0
    }
}

impl eframe::App for PaintApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Intercept the window close when there are unsaved changes.
        if ctx.input(|i| i.viewport().close_requested())
            && self.document.modified
            && !self.force_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_close_confirm = true;
        }

        self.handle_shortcuts(ctx);
        self.update_pick_hold(ctx);
        self.update_title(ctx);

        ui::menubar::show(self, ctx);
        ui::toolbox::show(self, ctx);
        ui::status_bar::show(self, ctx);
        ui::colorbox::show(self, ctx);
        ui::color_picker::show(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui::canvas_view::show(self, ui);
        });

        if self.show_about {
            egui::Window::new("About RS Paint")
                .collapsible(false)
                .resizable(false)
                .open(&mut self.show_about)
                .show(ctx, |ui| {
                    ui.label("RS Paint");
                    ui.label("A Rust port of JS Paint.");
                    ui.label("Licensed under the MIT license.");
                });
        }

        self.show_close_confirm_window(ctx);
        self.show_attributes_window(ctx);
        self.show_history_window(ctx);
        self.show_flip_rotate_window(ctx);
        self.show_stretch_skew_window(ctx);
        self.show_error_window(ctx);
    }
}

impl PaintApp {
    fn show_attributes_window(&mut self, ctx: &egui::Context) {
        if !self.show_attributes {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Attributes")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                egui::Grid::new("attributes-grid").show(ui, |ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut self.attributes_width).range(1..=10_000));
                    ui.end_row();
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut self.attributes_height).range(1..=10_000));
                    ui.end_row();
                });
                ui.checkbox(&mut self.attributes_transparent, "Transparent");
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.apply_attributes();
        } else if cancel || !open {
            self.show_attributes = false;
        }
    }

    fn show_history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history {
            return;
        }
        let nodes: Vec<(rspaint_core::NodeId, usize, String)> = self
            .history
            .nodes()
            .iter()
            .map(|node| {
                (
                    node.id,
                    history_depth(&self.history, node.id),
                    node.name.clone(),
                )
            })
            .collect();
        let current = self.history.current();
        let mut open = true;
        let mut jump = None;
        egui::Window::new("History")
            .default_size([240.0, 320.0])
            .open(&mut open)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (id, depth, name) in &nodes {
                        let label = format!("{}{name}", "    ".repeat(*depth));
                        if ui.selectable_label(*id == current, label).clicked() {
                            jump = Some(*id);
                        }
                    }
                });
            });
        if let Some(id) = jump {
            self.jump_to_history(id);
        }
        if !open {
            self.show_history = false;
        }
    }

    fn show_flip_rotate_window(&mut self, ctx: &egui::Context) {
        if !self.show_flip_rotate {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Flip and Rotate")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.radio_value(
                    &mut self.flip_rotate,
                    FlipRotate::FlipHorizontal,
                    "Flip horizontal",
                );
                ui.radio_value(
                    &mut self.flip_rotate,
                    FlipRotate::FlipVertical,
                    "Flip vertical",
                );
                ui.radio_value(&mut self.flip_rotate, FlipRotate::Rotate90, "Rotate 90°");
                ui.radio_value(&mut self.flip_rotate, FlipRotate::Rotate180, "Rotate 180°");
                ui.radio_value(&mut self.flip_rotate, FlipRotate::Rotate270, "Rotate 270°");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.flip_rotate, FlipRotate::Arbitrary, "Angle:");
                    ui.add(
                        egui::DragValue::new(&mut self.flip_rotate_angle)
                            .range(-360.0..=360.0)
                            .suffix("°"),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.apply_flip_rotate();
        } else if cancel || !open {
            self.show_flip_rotate = false;
        }
    }

    fn show_stretch_skew_window(&mut self, ctx: &egui::Context) {
        if !self.show_stretch_skew {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Stretch and Skew")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Stretch").strong());
                percent_row(ui, "Horizontal:", &mut self.stretch_x);
                percent_row(ui, "Vertical:", &mut self.stretch_y);
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Skew").strong());
                angle_row(ui, "Horizontal:", &mut self.skew_h);
                angle_row(ui, "Vertical:", &mut self.skew_v);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.apply_stretch_skew();
        } else if cancel || !open {
            self.show_stretch_skew = false;
        }
    }

    /// The "unsaved changes" confirmation shown when closing the window.
    fn show_close_confirm_window(&mut self, ctx: &egui::Context) {
        if !self.show_close_confirm {
            return;
        }
        let mut open = true;
        let mut action = None;
        egui::Window::new("Cerrar RS Paint")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.add_space(8.0);
                ui.label("¿Seguro que quieres cerrar el programa?");
                ui.label("Los cambios no guardados se perderán.");
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        action = Some(Action::Cancel);
                    }
                    if ui.button("Guardar y salir").clicked() {
                        action = Some(Action::SaveAndClose);
                    }
                    if ui.button("Salir").clicked() {
                        action = Some(Action::Exit);
                    }
                });
                ui.add_space(4.0);
            });
        if !open {
            action = Some(Action::Cancel);
        }

        match action {
            Some(Action::Cancel) => self.show_close_confirm = false,
            Some(Action::SaveAndClose) => {
                self.save();
                if !self.document.modified {
                    self.force_close = true;
                    self.show_close_confirm = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Some(Action::Exit) => {
                self.force_close = true;
                self.show_close_confirm = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
    }

    fn show_error_window(&mut self, ctx: &egui::Context) {
        let Some(message) = self.error_message.clone() else {
            return;
        };
        let mut dismiss = false;
        egui::Window::new("Error")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(message);
                if ui.button("OK").clicked() {
                    dismiss = true;
                }
            });
        if dismiss {
            self.error_message = None;
        }
    }
}

/// Adds `delta` to a tool size, clamped to `1..=max`.
fn step_size(value: &mut u32, delta: i32, max: u32) {
    let next = (*value as i64 + delta as i64).clamp(1, max as i64);
    *value = next as u32;
}

/// The depth of a node in the history tree (0 for the root).
fn history_depth(history: &History, id: rspaint_core::NodeId) -> usize {
    let mut depth = 0;
    let mut current = id;
    while let Some(parent) = history.node(current).and_then(|node| node.parent) {
        depth += 1;
        current = parent;
    }
    depth
}

/// Fill style for the Rectangle, Ellipse and Rounded Rectangle tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeFill {
    /// Outline only, in the stroke color.
    Outline,
    /// Outline in the stroke color, interior filled with the other color.
    FilledWithOutline,
    /// Interior filled with the stroke color, no outline.
    FilledOnly,
}

/// Where the Outliner draws its border relative to the detected contour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BorderPlacement {
    /// Outside the contour (on the background side).
    Outside,
    /// First pass paints one pixel inside the contour, then grows outward.
    OneInward,
    /// Grows both outward and inward.
    Symmetric,
}

/// The operation chosen in the Flip/Rotate dialog.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlipRotate {
    FlipHorizontal,
    FlipVertical,
    Rotate90,
    Rotate180,
    Rotate270,
    Arbitrary,
}

/// File actions triggered by keyboard shortcuts.
enum FileAction {
    Save,
    SaveAs,
    Open,
}

/// A labeled percentage input (1–500%).
fn percent_row(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(1.0..=500.0).suffix("%"));
    });
}

/// A labeled skew angle input in degrees (-89–89°).
fn angle_row(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(-89.0..=89.0).suffix("°"));
    });
}

/// Constrains a box shape to a square, using the larger cursor delta.
fn square_end(start: (i32, i32), p: (i32, i32)) -> (i32, i32) {
    let dx = p.0 - start.0;
    let dy = p.1 - start.1;
    let size = dx.abs().max(dy.abs());
    let sx = if dx < 0 { -1 } else { 1 };
    let sy = if dy < 0 { -1 } else { 1 };
    (start.0 + sx * size, start.1 + sy * size)
}

/// Snaps a line to the nearest multiple of 45°, keeping the cursor distance
/// (JS Paint's Shift behavior for the Line and Curve tools).
fn snap_to_45(start: (i32, i32), p: (i32, i32)) -> (i32, i32) {
    let dx = (p.0 - start.0) as f64;
    let dy = (p.1 - start.1) as f64;
    if dx == 0.0 && dy == 0.0 {
        return start;
    }
    let eighth_turn = std::f64::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / eighth_turn).round() * eighth_turn;
    let distance = (dx * dx + dy * dy).sqrt();
    (
        start.0 + (angle.cos() * distance).round() as i32,
        start.1 + (angle.sin() * distance).round() as i32,
    )
}

/// Builds a selection from a freshly created region.
fn selection_from_region(region: NewRegion) -> Selection {
    let width = region.pixels.width();
    let height = region.pixels.height();
    let boundary = if region.mask {
        rspaint_core::selection::boundary_segments(&region.mask_bits, width, height)
    } else {
        Vec::new()
    };
    let runs = runs_from_mask(&region.mask_bits, width, height);
    Selection {
        pixels: region.pixels,
        x: region.x,
        y: region.y,
        outline: region.outline,
        mask: region.mask,
        boundary,
        mask_bits: region.mask_bits,
        runs,
        floating: false,
        dragging: false,
        moved: false,
        drag_offset: (0, 0),
        base: None,
        name: region.name,
    }
}

/// Renders a selection mask as an opaque-white buffer (for transforming it).
fn mask_to_buffer(mask_bits: &[bool], width: u32, height: u32) -> PixelBuffer {
    let mut buffer = PixelBuffer::transparent(width, height);
    for y in 0..height {
        for x in 0..width {
            if mask_bits
                .get((y * width + x) as usize)
                .copied()
                .unwrap_or(false)
            {
                buffer.set(x, y, Color::WHITE);
            }
        }
    }
    buffer
}

/// Reads a selection mask back from a buffer's alpha channel.
fn buffer_to_mask(buffer: &PixelBuffer) -> Vec<bool> {
    let (width, height) = (buffer.width(), buffer.height());
    let mut mask = vec![false; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            if buffer.get(x, y).is_some_and(|color| color.a > 127) {
                mask[(y * width + x) as usize] = true;
            }
        }
    }
    mask
}

/// Horizontal runs `(y, x_start, x_end)` of a mask (inclusive `x_end`).
fn runs_from_mask(mask_bits: &[bool], width: u32, height: u32) -> Vec<HighlightRun> {
    let mut runs = Vec::new();
    let w = width as i32;
    for y in 0..height as i32 {
        let mut x = 0;
        while x < w {
            let idx = (y as u32 * width + x as u32) as usize;
            if mask_bits.get(idx).copied().unwrap_or(false) {
                let start = x;
                while x < w
                    && mask_bits
                        .get((y as u32 * width + x as u32) as usize)
                        .copied()
                        .unwrap_or(false)
                {
                    x += 1;
                }
                runs.push((y, start, x - 1));
            } else {
                x += 1;
            }
        }
    }
    runs
}

/// Clears the pixels where `mask_bits` is true, at `(x, y)`.
fn clear_region_masked(
    dst: &mut PixelBuffer,
    mask_bits: &[bool],
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    fill: Color,
) {
    for oy in 0..height as i32 {
        for ox in 0..width as i32 {
            if mask_bits
                .get((oy as u32 * width + ox as u32) as usize)
                .copied()
                .unwrap_or(false)
            {
                let dx = x + ox;
                let dy = y + oy;
                if dx >= 0 && dy >= 0 {
                    dst.set(dx as u32, dy as u32, fill);
                }
            }
        }
    }
}

/// Whether `region` overlaps any pixel already selected in `previous`.
fn region_removes_any(previous: &Selection, region: &NewRegion) -> bool {
    let (px, py) = (previous.x, previous.y);
    let (pw, ph) = (previous.pixels.width(), previous.pixels.height());
    let rw = region.pixels.width();
    for oy in 0..region.pixels.height() as i32 {
        for ox in 0..rw as i32 {
            if !region
                .mask_bits
                .get((oy as u32 * rw + ox as u32) as usize)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            let lx = region.x + ox - px;
            let ly = region.y + oy - py;
            let selected = lx >= 0
                && ly >= 0
                && lx < pw as i32
                && ly < ph as i32
                && previous
                    .mask_bits
                    .get((ly as u32 * pw + lx as u32) as usize)
                    .copied()
                    .unwrap_or(true);
            if selected {
                return true;
            }
        }
    }
    false
}

/// Subtracts a region from a selection, returning `None` if nothing remains.
fn subtract_region(previous: Selection, region: &NewRegion) -> Option<Selection> {
    let (ax, ay) = (previous.x, previous.y);
    let (aw, ah) = (previous.pixels.width(), previous.pixels.height());
    let mut mask = if previous.mask_bits.is_empty() {
        vec![true; (aw * ah) as usize]
    } else {
        previous.mask_bits.clone()
    };
    let mut content = previous.pixels.clone();
    let rw = region.pixels.width();
    for oy in 0..region.pixels.height() as i32 {
        for ox in 0..rw as i32 {
            if !region
                .mask_bits
                .get((oy as u32 * rw + ox as u32) as usize)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            let lx = region.x + ox - ax;
            let ly = region.y + oy - ay;
            if lx >= 0 && ly >= 0 && lx < aw as i32 && ly < ah as i32 {
                mask[(ly as u32 * aw + lx as u32) as usize] = false;
                content.set(lx as u32, ly as u32, Color::TRANSPARENT);
            }
        }
    }
    if !mask.iter().any(|&selected| selected) {
        return None;
    }
    let boundary = rspaint_core::selection::boundary_segments(&mask, aw, ah);
    let runs = runs_from_mask(&mask, aw, ah);
    Some(Selection {
        pixels: content,
        x: ax,
        y: ay,
        outline: Vec::new(),
        mask: true,
        boundary,
        mask_bits: mask,
        runs,
        floating: previous.floating,
        dragging: false,
        moved: previous.moved,
        drag_offset: (0, 0),
        base: previous.base,
        name: previous.name,
    })
}

/// Whether `region` includes any pixel not already selected in `previous`.
fn region_adds_new(previous: &Selection, region: &NewRegion) -> bool {
    let (px, py) = (previous.x, previous.y);
    let (pw, ph) = (previous.pixels.width(), previous.pixels.height());
    let rw = region.pixels.width();
    for oy in 0..region.pixels.height() as i32 {
        for ox in 0..rw as i32 {
            if !region
                .mask_bits
                .get((oy as u32 * rw + ox as u32) as usize)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            let lx = region.x + ox - px;
            let ly = region.y + oy - py;
            let already = lx >= 0
                && ly >= 0
                && lx < pw as i32
                && ly < ph as i32
                && previous
                    .mask_bits
                    .get((ly as u32 * pw + lx as u32) as usize)
                    .copied()
                    .unwrap_or(true);
            if !already {
                return true;
            }
        }
    }
    false
}

/// Unions a previous selection with a new region, preserving a floating move.
fn union_region(previous: Selection, region: NewRegion, fill: Color) -> Selection {
    let (ax, ay) = (previous.x, previous.y);
    let (aw, ah) = (previous.pixels.width(), previous.pixels.height());
    let (bx, by) = (region.x, region.y);
    let (bw, bh) = (region.pixels.width(), region.pixels.height());
    let min_x = ax.min(bx);
    let min_y = ay.min(by);
    let max_x = (ax + aw as i32).max(bx + bw as i32);
    let max_y = (ay + ah as i32).max(by + bh as i32);
    let width = (max_x - min_x) as u32;
    let height = (max_y - min_y) as u32;

    let mut merged = PixelBuffer::transparent(width, height);
    let mut merged_mask = vec![false; (width * height) as usize];

    for oy in 0..ah as i32 {
        for ox in 0..aw as i32 {
            let source = (oy as u32 * aw + ox as u32) as usize;
            if previous.mask_bits.get(source).copied().unwrap_or(true) {
                let dx = (ax - min_x + ox) as u32;
                let dy = (ay - min_y + oy) as u32;
                merged_mask[(dy * width + dx) as usize] = true;
                if let Some(color) = previous.pixels.get(ox as u32, oy as u32) {
                    if color.a > 0 {
                        merged.set(dx, dy, color);
                    }
                }
            }
        }
    }
    for oy in 0..bh as i32 {
        for ox in 0..bw as i32 {
            let source = (oy as u32 * bw + ox as u32) as usize;
            if region.mask_bits.get(source).copied().unwrap_or(false) {
                let dx = (bx - min_x + ox) as u32;
                let dy = (by - min_y + oy) as u32;
                merged_mask[(dy * width + dx) as usize] = true;
                if let Some(color) = region.pixels.get(ox as u32, oy as u32) {
                    if color.a > 0 {
                        merged.set(dx, dy, color);
                    }
                }
            }
        }
    }

    // If the previous selection was floating, keep it floating with an updated
    // base that also clears the newly added region.
    let floating = previous.floating;
    let base = if floating {
        previous.base.clone().map(|mut base| {
            clear_region_masked(&mut base, &region.mask_bits, bx, by, bw, bh, fill);
            base
        })
    } else {
        None
    };

    let boundary = rspaint_core::selection::boundary_segments(&merged_mask, width, height);
    let runs = runs_from_mask(&merged_mask, width, height);
    Selection {
        pixels: merged,
        x: min_x,
        y: min_y,
        outline: Vec::new(),
        mask: true,
        boundary,
        mask_bits: merged_mask,
        runs,
        floating,
        dragging: false,
        moved: previous.moved,
        drag_offset: (0, 0),
        base,
        name: region.name,
    }
}

/// The bounding box covering both the old and new positions of a `width` x
/// `height` selection.
fn union_rect(old_x: i32, old_y: i32, new_x: i32, new_y: i32, width: u32, height: u32) -> RectI {
    let last = (width as i32 - 1, height as i32 - 1);
    let old = RectI::between((old_x, old_y), (old_x + last.0, old_y + last.1));
    let new = RectI::between((new_x, new_y), (new_x + last.0, new_y + last.1));
    old.union(new)
}

fn file_stem(path: &std::path::Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
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

#[cfg(test)]
mod tests {
    use super::{snap_to_45, square_end};

    #[test]
    fn square_end_equalizes_sides() {
        assert_eq!(square_end((0, 0), (10, 3)), (10, 10));
        assert_eq!(square_end((0, 0), (3, -10)), (10, -10));
        assert_eq!(square_end((5, 5), (5, 5)), (5, 5));
    }

    #[test]
    fn snap_to_45_snaps_to_eight_directions() {
        assert_eq!(snap_to_45((0, 0), (10, 1)), (10, 0), "near horizontal");
        assert_eq!(snap_to_45((0, 0), (5, 5)), (5, 5), "exactly diagonal");
        assert_eq!(snap_to_45((0, 0), (1, 10)), (0, 10), "near vertical");
        assert_eq!(snap_to_45((0, 0), (-5, 5)), (-5, 5), "up-left diagonal");
    }
}

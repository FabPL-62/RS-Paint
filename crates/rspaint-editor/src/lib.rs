//! Headless editing component. Owns document operations and tool state.
//! No windows, widgets, GPU resources, file dialogs or system clipboard.
#![forbid(unsafe_code)]

use emath::{self as geometry};
use rspaint_core::draw::{
    draw_bezier, draw_ellipse, draw_line, draw_line_masked, draw_quadratic_curve, draw_rectangle,
    draw_rounded_rectangle, erase_line, fill_ellipse, fill_rectangle, fill_rounded_rectangle,
    walk_line,
};
use rspaint_core::fill::flood_fill;
use rspaint_core::{
    palette, transform, BlendMode, Brush, BrushShape, Color, Document, History, HistorySnapshot,
    LayerBlend, LayerNode, LayerRow, PixelBuffer,
};

pub mod text;
pub mod tools;
use tools::Tool;

pub const MIN_MAGNIFICATION: f32 = 0.125;
pub const MAX_MAGNIFICATION: f32 = 64.0;
const MAX_ROUND_RADIUS: u32 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Extra1,
    Extra2,
}

#[derive(Clone, Debug)]
pub struct Upload {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// RGBA8 bytes, shared with the document buffer when uploading it whole.
    pub data: std::sync::Arc<Vec<u8>>,
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
    /// Original blocks touched by the stroke, for cancellation and previews.
    base: Option<PreviewBackup>,
    /// The bounding box of the last shape preview, restored before the next one.
    preview_rect: Option<RectI>,
}

/// A horizontal selection highlight run `(y, x_start, x_end)`, inclusive.
pub type HighlightRun = (i32, i32, i32);

/// The cached outline and highlight runs of one tile of an in-progress
/// Selection Brush stroke. Coordinates are absolute canvas pixels.
///
/// The mask is split into tiles so a stroke only recomputes the tiles it
/// touches, instead of rescanning the whole canvas on every pointer move.
#[derive(Default)]
pub struct SelectionBrushTile {
    /// Canvas bounds [left, top, right, bottom], exclusive end.
    pub bounds: [i32; 4],
    /// Non-overlapping highlight rectangles [left, top, right, bottom], exclusive end.
    pub highlight_rects: Vec<[i32; 4]>,
    pub boundary: Vec<rspaint_core::selection::BoundarySegment>,
    pub runs: Vec<HighlightRun>,
}

/// A selection undo snapshot: the selection, the canvas and the rotation pivot.
type SelectionUndo = (Option<Selection>, PixelBuffer, Option<(f32, f32)>);

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
    /// Unrotated content for repeated rotations: `(pixels, mask, x, y)`.
    rotation_origin: Option<(PixelBuffer, Vec<bool>, i32, i32)>,
    /// The rotation handle's angle from "up" (radians).
    rotation: f32,
    /// Set for a freshly pasted element: it keeps its transform controls and
    /// moves its content on a plain drag (as if Ctrl was still held) until it
    /// is dropped, so pressing outside it commits it where it is.
    pasted: bool,
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
    /// Every layer's pixels before the drag started.
    orig_layers: Vec<LayerNode>,
    orig_active: u64,
    orig_width: u32,
    orig_height: u32,
    /// The canvas rectangle's top-left at the start of the drag (screen space).
    start_min: geometry::Pos2,
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
    start_min: geometry::Pos2,
    magnification: f32,
}

/// An in-progress rotation of the selection content around a pivot.
struct SelectionRotateDrag {
    /// The pointer angle from "up" when the drag started.
    start_angle: f32,
    /// The selection's rotation angle when the drag started.
    base_angle: f32,
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
    pub underline: bool,
    pub vertical: bool,
    pub transparent: bool,
}

/// Which color a color picker edits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickTarget {
    Primary,
    Secondary,
}

/// Document, tool settings and in-progress editing operations.
pub struct Editor {
    /// UI configures these options; operations remain owned by the editor.
    pub settings: ToolSettings,
    pub colors: DocumentColors,
    pub document: Document,
    pub history: History<HistorySnapshot>,
    pub magnification: f32,
    pub pan: geometry::Vec2,
    pub file_path: Option<std::path::PathBuf>,
    /// The layer rows selected in the panel. Always non-empty once a document
    /// exists; it may not contain the active layer after a Ctrl+click.
    pub layer_selection: Vec<u64>,
    /// The row a Shift+click extends the selection from.
    pub layer_anchor: Option<u64>,
    pub selection: Option<Selection>,
    /// The other selected areas while one of them is being moved on its own.
    /// They are merged back into `selection` when the drag ends.
    selection_rest: Option<Selection>,
    /// Whether the current selection drag also moves the pixels underneath.
    selection_drag_moves_content: bool,
    creating_selection: Option<CreatingSelection>,
    creating_selection_add: bool,
    creating_selection_subtract: bool,
    selection_undo: Vec<SelectionUndo>,
    clipboard: Option<ClipboardContent>,
    resize_drag: Option<ResizeDrag>,
    selection_resize: Option<SelectionResizeDrag>,
    zoom_rect: Option<((i32, i32), (i32, i32))>,
    selection_pivot: Option<(f32, f32)>,
    selection_pivot_dragging: bool,
    selection_rotate: Option<SelectionRotateDrag>,
    viewport_size: geometry::Vec2,
    pending_adjust_zoom: bool,
    polygon_points: Vec<(i32, i32)>,
    polygon_preview: Option<(i32, i32)>,
    polygon_stroke: Color,
    polygon_fill: Color,
    magic_wand_seed: Option<(i32, i32)>,
    sel_brush_mask: Vec<bool>,
    sel_brush_subtract: bool,
    sel_brush_active: bool,
    sel_brush_last: Option<(i32, i32)>,
    curve_points: Vec<(i32, i32)>,
    curve_base: Option<PreviewBackup>,
    curve_preview_rect: Option<RectI>,
    curve_dragging: bool,
    fonts: crate::text::FontSet,
    text_box: Option<TextBox>,
    text_buffer: String,
    text_creating: Option<((i32, i32), (i32, i32))>,
    text_base: Option<PreviewBackup>,
    sel_brush_tiles: Vec<SelectionBrushTile>,
    sel_brush_tile_cols: i32,
    pub pick_hold: Option<PickTarget>,
    pub pointer: Option<(i32, i32)>,
    stroke: Option<Stroke>,
    stroke_painted: Vec<bool>,
    /// Last two kept pixels of a pixel-perfect 1 px stroke, and whether the
    /// last one was painted by this stroke (rather than already being there).
    thin_prev2: Option<(i32, i32)>,
    thin_prev1: Option<(i32, i32)>,
    thin_prev1_new: bool,
    /// Smoothed freehand pointer position and the raw point it catches up to.
    smooth_position: Option<(f32, f32)>,
    smooth_target: Option<(i32, i32)>,
    rng: Rng,
    canvas_dirty: bool,
    /// A full upload is pending, but history restoration already composed it.
    full_upload_ready: bool,
    dirty_rect: Option<RectI>,
    dirty_regions: Vec<RectI>,
    revision: u64,
}
/// Values configured by controls. No widget or transient operation state.
/// The workspace copies these settings when switching documents.
#[derive(Clone)]
pub struct ToolSettings {
    pub tool: Tool,
    pub brush_shape: BrushShape,
    pub brush_size: u32,
    pub eraser_size: u32,
    pub eraser_shape: BrushShape,
    pub airbrush_size: u32,
    pub pencil_size: u32,
    pub stroke_size: u32,
    pub round_radius: u32,
    pub fill_style: ShapeFill,
    pub paint_blend: BlendMode,
    pub grid_opacity: u32,
    pub show_grid: bool,
    pub outliner_size: u32,
    pub outliner_concave: bool,
    pub outliner_color: Color,
    pub outliner_placement: BorderPlacement,
    pub selection_transparent: bool,
    /// Removes the extra pixel at the corners of 1 px pencil/brush strokes.
    pub pixel_perfect: bool,
    /// Pointer smoothing for Pencil and Brush, from 0 (off) to 100 (slowest).
    pub stroke_smoothing: u8,
    pub magic_wand_tolerance: u8,
    pub text_family: crate::text::FontFamily,
    pub text_size: f32,
    pub text_bold: bool,
    pub text_italic: bool,
    pub text_underline: bool,
    pub text_vertical: bool,
    pub text_transparent: bool,
}

/// Color choices belong to one document and are never copied by Shared.
pub struct DocumentColors {
    pub foreground: Color,
    pub background: Color,
    pub palette: Vec<Color>,
    pub captured: Vec<Color>,
}

impl Default for DocumentColors {
    fn default() -> Self {
        Self {
            foreground: Color::BLACK,
            background: Color::WHITE,
            palette: palette::default_palette(),
            captured: vec![Color::BLACK; 28],
        }
    }
}

pub type Shared = ToolSettings;

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            tool: Tool::Pencil,
            brush_shape: BrushShape::Circle,
            brush_size: 4,
            eraser_size: 8,
            eraser_shape: BrushShape::Square,
            airbrush_size: 9,
            pencil_size: 1,
            stroke_size: 1,
            round_radius: 10,
            fill_style: ShapeFill::Outline,
            paint_blend: BlendMode::Blend,
            grid_opacity: 20,
            show_grid: true,
            outliner_size: 1,
            outliner_concave: false,
            outliner_color: Color::WHITE,
            outliner_placement: BorderPlacement::Outside,
            selection_transparent: false,
            pixel_perfect: false,
            stroke_smoothing: 0,
            magic_wand_tolerance: 0,
            text_family: crate::text::FontFamily::Sans,
            text_size: 16.0,
            text_bold: false,
            text_italic: false,
            text_underline: false,
            text_vertical: false,
            text_transparent: true,
        }
    }
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

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
impl Editor {
    pub fn new() -> Self {
        let mut document = Document::default();
        let history = History::new(document.history_snapshot(None), "New");
        let layer_selection = vec![document.active];
        Self {
            settings: ToolSettings::default(),
            colors: DocumentColors::default(),
            document,
            history,
            magnification: 1.0,
            pan: geometry::Vec2::ZERO,
            file_path: None,
            layer_selection,
            layer_anchor: None,
            selection: None,
            selection_rest: None,
            selection_drag_moves_content: true,
            creating_selection: None,
            creating_selection_add: false,
            creating_selection_subtract: false,
            selection_undo: Vec::new(),
            clipboard: None,
            resize_drag: None,
            selection_resize: None,
            zoom_rect: None,
            selection_pivot: None,
            selection_pivot_dragging: false,
            selection_rotate: None,
            viewport_size: geometry::Vec2::ZERO,
            pending_adjust_zoom: true,
            polygon_points: Vec::new(),
            polygon_preview: None,
            polygon_stroke: Color::BLACK,
            polygon_fill: Color::WHITE,
            magic_wand_seed: None,
            sel_brush_mask: Vec::new(),
            sel_brush_subtract: false,
            sel_brush_active: false,
            sel_brush_last: None,
            curve_points: Vec::new(),
            curve_base: None,
            curve_preview_rect: None,
            curve_dragging: false,
            fonts: crate::text::FontSet::load(),
            text_box: None,
            text_buffer: String::new(),
            text_creating: None,
            text_base: None,
            sel_brush_tiles: Vec::new(),
            sel_brush_tile_cols: 0,
            pick_hold: None,
            pointer: None,
            stroke: None,
            stroke_painted: Vec::new(),
            thin_prev2: None,
            thin_prev1: None,
            thin_prev1_new: false,
            smooth_position: None,
            smooth_target: None,
            rng: Rng(0x1234_5678),
            canvas_dirty: true,
            full_upload_ready: false,
            dirty_rect: None,
            dirty_regions: Vec::new(),
            revision: 0,
        }
    }
}
impl Editor {
    /// Changes whenever the pixel presentation needs refreshing.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// True only for a fresh, unedited white document with no operation in progress.
    pub fn is_pristine_new_document(&self) -> bool {
        self.file_path.is_none()
            && !self.document.modified
            && !self.document.transparent
            && self.history.nodes().len() == 1
            && self.history.current_name() == "New"
            && self.document.width() == rspaint_core::document::DEFAULT_CANVAS_WIDTH
            && self.document.height() == rspaint_core::document::DEFAULT_CANVAS_HEIGHT
            && self.selection.is_none()
            && self.selection_undo.is_empty()
            && self.creating_selection.is_none()
            && self.stroke.is_none()
            && self.resize_drag.is_none()
            && self.selection_resize.is_none()
            && self.selection_rotate.is_none()
            && self.polygon_points.is_empty()
            && self.curve_points.is_empty()
            && self.text_box.is_none()
            && self.text_creating.is_none()
            && !self.sel_brush_active
            && self
                .document
                .buffer
                .data()
                .chunks_exact(4)
                .all(|pixel| pixel == [255, 255, 255, 255])
    }

    pub fn selection_pixels(&self) -> Option<&PixelBuffer> {
        self.selection.as_ref().map(|s| &s.pixels)
    }
    pub fn curve_points(&self) -> &[(i32, i32)] {
        &self.curve_points
    }
    pub fn copy_selection(&mut self) -> Option<PixelBuffer> {
        let selection = self.selection.as_ref()?;
        self.clipboard = Some(ClipboardContent {
            pixels: selection.pixels.clone(),
            mask_bits: selection.mask_bits.clone(),
            outline: selection.outline.clone(),
            mask: selection.mask,
        });
        Some(selection.pixels.clone())
    }

    pub fn cut_selection(&mut self) -> Option<PixelBuffer> {
        let pixels = self.copy_selection()?;
        self.delete_selection_named("Cut Selection");
        Some(pixels)
    }

    /// Records a successful save. Encoding and filesystem access are adapters.
    pub fn mark_saved(&mut self, path: std::path::PathBuf) {
        self.document.name = file_stem(&path).unwrap_or_else(|| "Untitled".to_owned());
        self.file_path = Some(path);
        self.document.modified = false;
    }
    pub fn paste_clipboard(&mut self, external: Option<PixelBuffer>) {
        let content = if let Some(pixels) = external {
            let count = (pixels.width() * pixels.height()) as usize;
            (pixels, vec![true; count], Vec::new(), false)
        } else if let Some(c) = self.clipboard.clone() {
            (c.pixels, c.mask_bits, c.outline, c.mask)
        } else {
            return;
        };
        self.paste_pixels(content.0, content.1, content.2, content.3, "Paste");
    }
}

mod helpers;
use helpers::*;
mod drawing;
mod preview_backup;
use preview_backup::{OriginalPixels, PreviewBackup};
mod layers;
mod selection;
mod selection_brush;
mod selection_transform;
mod settings;
mod text_edit;
mod transforms;
mod viewport;

#[cfg(test)]
mod geometry_tests;

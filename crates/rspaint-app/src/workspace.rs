//! The multi-document workspace: a list of tabs (one editor each) plus the
//! cross-tab shared state, drag-to-reorder, drag-between-windows and tear-off.

use eframe::egui;

use crate::app::{PaintApp, Shared};

/// A pending action requested by the tab bar in the current frame.
enum TabAction {
    None,
    Switch(usize),
    Close(usize),
    New,
    /// Move the tab at `.0` to position `.1`.
    Reorder(usize, usize),
    /// Detach the tab at `.0` into a new window (dropped below the tab strip).
    TearOff(usize, egui::Pos2),
}

/// A tab drag in progress, shared across windows so a tab can be dropped onto
/// another window.
struct Drag {
    /// The window the drag started in.
    source: egui::ViewportId,
    /// The dragged tab's index in the source window.
    index: usize,
    /// The tab label (for the drag ghost).
    label: String,
    /// The last known pointer position.
    pos: egui::Pos2,
    /// Whether the pointer has left the source window.
    left: bool,
}

/// What a window wants the workspace to do after rendering.
struct RenderResult {
    closed: bool,
    /// A tab to detach into a new window: `(index, drop position)`.
    tear: Option<(usize, egui::Pos2)>,
}

/// One OS window: a list of tabs plus the per-window tab-drag state.
struct WindowState {
    id: egui::ViewportId,
    /// Id used to pick the per-window canvas GPU resources.
    render_id: u64,
    tabs: Vec<PaintApp>,
    active: usize,
    pending: TabAction,
    /// Whether the pointer is currently over this window.
    pointer_hover: bool,
    /// Whether the unsaved-changes confirmation dialog is open.
    confirm_close: bool,
    /// Set once the user confirmed closing, to bypass the confirmation.
    force_close: bool,
}

impl WindowState {
    fn new(id: egui::ViewportId, render_id: u64, tab: PaintApp) -> Self {
        Self {
            id,
            render_id,
            tabs: vec![tab],
            active: 0,
            pending: TabAction::None,
            pointer_hover: false,
            confirm_close: false,
            force_close: false,
        }
    }

    fn new_tab(&mut self, ctx: &egui::Context, has_gpu: bool, srgb: bool, shared: &Shared) {
        let mut tab = PaintApp::new_blank(has_gpu, srgb, ctx);
        tab.apply_shared(shared);
        tab.invalidate_canvas();
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
    }
}

/// Owns all windows and the shared cross-tab state.
pub struct Workspace {
    windows: Vec<WindowState>,
    shared: Shared,
    has_gpu: bool,
    srgb_output: bool,
    next_viewport: u64,
    drag: Option<Drag>,
}

impl Workspace {
    /// Creates the workspace with a single empty tab in the main window.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let first = PaintApp::new(cc);
        let shared = first.capture_shared();
        let has_gpu = first.has_gpu();
        let srgb_output = first.srgb_output();
        let main = WindowState::new(egui::ViewportId::ROOT, 0, first);
        Self {
            windows: vec![main],
            shared,
            has_gpu,
            srgb_output,
            next_viewport: 1,
            drag: None,
        }
    }

    /// Draws a window's tab bar and editor.
    fn render_window(
        ctx: &egui::Context,
        window: &mut WindowState,
        shared: &mut Shared,
        drag: &mut Option<Drag>,
        has_gpu: bool,
        srgb: bool,
    ) -> RenderResult {
        window.pointer_hover = ctx.input(|i| i.pointer.hover_pos().is_some());

        // Intercept the window close when any of its tabs has unsaved changes.
        if ctx.input(|i| i.viewport().close_requested()) {
            let modified = window.tabs.iter().any(|tab| tab.is_modified());
            if modified && !window.force_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                window.confirm_close = true;
            }
        }

        // Apply the action requested by the tab bar on the previous frame.
        let mut tear = None;
        match std::mem::replace(&mut window.pending, TabAction::None) {
            TabAction::Switch(index) if index < window.tabs.len() => {
                *shared = window.tabs[window.active].capture_shared();
                window.active = index;
                window.tabs[index].invalidate_canvas();
            }
            TabAction::Close(index) => window.close_tab(index),
            TabAction::New => window.new_tab(ctx, has_gpu, srgb, shared),
            TabAction::Reorder(from, to) => {
                if from < window.tabs.len() && to < window.tabs.len() && from != to {
                    let tab = window.tabs.remove(from);
                    window.tabs.insert(to, tab);
                    window.active = to;
                }
            }
            TabAction::TearOff(index, pos) => tear = Some((index, pos)),
            _ => {}
        }

        if window.tabs.is_empty() {
            return RenderResult { closed: true, tear };
        }
        if window.active >= window.tabs.len() {
            window.active = window.tabs.len() - 1;
        }
        let active = window.active;
        window.tabs[active].set_window_id(window.render_id);
        window.tabs[active].apply_shared(shared);
        window.tabs[active].update_editor_top(ctx);
        // The tab strip sits below the menu, right of the tool box and above
        // the canvas: it is drawn between the two editor halves.
        show_tab_bar(ctx, window, drag);
        window.tabs[active].update_editor_bottom(ctx);
        show_window_close_confirm(ctx, window);
        *shared = window.tabs[active].capture_shared();
        RenderResult {
            closed: false,
            tear,
        }
    }

    /// Moves a tab out of the window at `source` into a brand-new window.
    fn tear_off(&mut self, source: usize, index: usize, _pos: egui::Pos2) {
        if source >= self.windows.len() || index >= self.windows[source].tabs.len() {
            return;
        }
        // The main window must keep at least one tab (closing it exits the app).
        if source == 0 && self.windows[0].tabs.len() <= 1 {
            return;
        }
        let mut tab = self.windows[source].tabs.remove(index);
        tab.invalidate_canvas();
        if self.windows[source].active > index {
            self.windows[source].active -= 1;
        }
        if self.windows[source].active >= self.windows[source].tabs.len() {
            self.windows[source].active = self.windows[source].tabs.len().saturating_sub(1);
        }

        let render_id = self.next_viewport;
        let id = egui::ViewportId::from_hash_of(format!("rs-paint-torn-{}", self.next_viewport));
        self.next_viewport += 1;
        self.windows.push(WindowState::new(id, render_id, tab));
    }

    /// Draws a torn-off window in its own viewport. Returns `true` if it closed.
    fn render_torn(&mut self, ctx: &egui::Context, index: usize) -> bool {
        let id = self.windows[index].id;
        let title = self.windows[index]
            .tabs
            .first()
            .map(|tab| tab.tab_label())
            .unwrap_or_default();
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("{title} - RS Paint"))
            .with_inner_size([900.0, 700.0])
            .with_min_inner_size([480.0, 320.0]);

        let mut window = self.windows.remove(index);
        let has_gpu = self.has_gpu;
        let srgb = self.srgb_output;
        let shared = &mut self.shared;
        let drag = &mut self.drag;
        let result = ctx.show_viewport_immediate(id, builder, |ctx, _class| {
            Workspace::render_window(ctx, &mut window, shared, drag, has_gpu, srgb)
        });
        if result.closed {
            return true;
        }
        self.windows.insert(index, window);
        if let Some((tab_index, pos)) = result.tear {
            self.tear_off(index, tab_index, pos);
        }
        false
    }

    /// Resolves an in-progress cross-window drag after all windows are drawn.
    fn resolve_drag(&mut self) {
        let Some((source_id, index, pos, left)) = self
            .drag
            .as_ref()
            .map(|d| (d.source, d.index, d.pos, d.left))
        else {
            return;
        };
        let Some(source) = self.windows.iter().position(|w| w.id == source_id) else {
            self.drag = None;
            return;
        };

        // Drop onto the window the pointer is hovering (if any, and not the
        // source), otherwise detach into a new window once the pointer leaves.
        let target = self
            .windows
            .iter()
            .position(|w| w.id != source_id && w.pointer_hover);
        if let Some(target) = target {
            // The main window must keep at least one tab (closing it exits).
            if !(source == 0 && self.windows[0].tabs.len() <= 1)
                && index < self.windows[source].tabs.len()
            {
                let mut tab = self.windows[source].tabs.remove(index);
                tab.invalidate_canvas();
                let src = &mut self.windows[source];
                if src.active > index {
                    src.active -= 1;
                }
                if src.active >= src.tabs.len() {
                    src.active = src.tabs.len().saturating_sub(1);
                }
                self.windows[target].tabs.push(tab);
                self.windows[target].active = self.windows[target].tabs.len() - 1;
            }
            self.drag = None;
        } else if left {
            self.tear_off(source, index, pos);
            self.drag = None;
        }
        // Otherwise the pointer is still inside the source: keep the drag.
    }
}

/// Draws a window's tab strip and records any requested action.
fn show_tab_bar(ctx: &egui::Context, window: &mut WindowState, drag: &mut Option<Drag>) {
    let panel_id = egui::Id::new(("tab-bar", window.id));
    let strip_fill = egui::Color32::from_gray(178);
    egui::TopBottomPanel::top(panel_id)
        .frame(
            egui::Frame::new()
                .fill(strip_fill)
                .inner_margin(egui::Margin {
                    left: 6,
                    right: 6,
                    top: 4,
                    bottom: 0,
                }),
        )
        .show(ctx, |ui| {
            let strip_bottom = ui.max_rect().bottom();
            let mut rects: Vec<egui::Rect> = Vec::new();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for index in 0..window.tabs.len() {
                    let label = window.tabs[index].tab_label();
                    let selected = index == window.active;
                    let (resp, close) = tab_widget(ui, &label, selected);
                    let rect = resp.rect;
                    rects.push(rect);
                    if close {
                        window.pending = TabAction::Close(index);
                    } else if resp.clicked() {
                        window.pending = TabAction::Switch(index);
                    }
                    if resp.drag_started() {
                        *drag = Some(Drag {
                            source: window.id,
                            index,
                            label: label.clone(),
                            pos: rect.center(),
                            left: false,
                        });
                    }
                    if resp.dragged() {
                        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                        if let Some(d) = drag.as_mut() {
                            if d.source == window.id {
                                if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
                                    d.pos = pos;
                                    d.left = !ctx.viewport_rect().contains(pos);
                                } else {
                                    d.left = true;
                                }
                            }
                        }
                    }
                    if resp.drag_stopped() {
                        if let Some(d) = drag.as_ref() {
                            if d.source == window.id {
                                // A release back inside the source window
                                // reorders instead of tearing off.
                                if !d.left {
                                    if let Some(pos) = resp.interact_pointer_pos() {
                                        if pos.y > strip_bottom + TEAR_OFF_DISTANCE {
                                            // Dropped below the strip: detach.
                                            window.pending = TabAction::TearOff(d.index, pos);
                                        } else {
                                            // Reorder by the position under the
                                            // pointer (count the tab centers to
                                            // its left).
                                            let from = d.index;
                                            let target = rects
                                                .iter()
                                                .filter(|r| r.center().x < pos.x)
                                                .count();
                                            let to =
                                                if target > from { target - 1 } else { target };
                                            if to != from {
                                                window.pending = TabAction::Reorder(from, to);
                                            }
                                        }
                                    }
                                }
                                *drag = None;
                            }
                        }
                    }
                }
                if ui.button("+").on_hover_text("New tab").clicked() {
                    window.pending = TabAction::New;
                }
            });
        });

    // Drag ghost: a floating copy of the tab following the cursor (only while
    // the pointer is still inside the source window; the OS shows nothing once
    // the cursor leaves the window).
    if let Some(d) = drag.as_ref() {
        if d.source == window.id && !d.left {
            paint_drag_ghost(ctx, &d.label);
        }
    }
}

/// Draws a floating copy of a tab at the cursor.
fn paint_drag_ghost(ctx: &egui::Context, label: &str) {
    let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else {
        return;
    };
    egui::Area::new(egui::Id::new("tab-drag-ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(10.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_gray(210))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(90)))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(label));
                });
        });
}

/// Shows the unsaved-changes confirmation for a window close request.
fn show_window_close_confirm(ctx: &egui::Context, window: &mut WindowState) {
    if !window.confirm_close {
        return;
    }
    let mut open = true;
    let mut action = 0u8;
    egui::Window::new("Cerrar ventana")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.label("Hay cambios sin guardar en esta ventana.");
            ui.label("¿Seguro que quieres cerrarla?");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancelar").clicked() {
                    action = 1;
                }
                if ui.button("Guardar y cerrar").clicked() {
                    action = 2;
                }
                if ui.button("Cerrar sin guardar").clicked() {
                    action = 3;
                }
            });
        });
    if !open {
        action = 1;
    }
    match action {
        1 => window.confirm_close = false,
        2 => {
            let mut all_saved = true;
            for tab in window.tabs.iter_mut() {
                if tab.is_modified() {
                    tab.save();
                    if tab.is_modified() {
                        all_saved = false;
                    }
                }
            }
            if all_saved {
                window.force_close = true;
                window.confirm_close = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        3 => {
            window.force_close = true;
            window.confirm_close = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        _ => {}
    }
}

/// The height of the tab strip (and of each tab).
const TAB_HEIGHT: f32 = 24.0;
/// The maximum width of a tab before its label is ellipsized.
const TAB_MAX_WIDTH: f32 = 200.0;
/// How far below the tab strip a tab must be dragged to detach it.
const TEAR_OFF_DISTANCE: f32 = 40.0;

/// Draws one tab: a rectangle with left/top/right borders, an ellipsized
/// label and a close button. Returns the tab's response and whether its close
/// button was clicked.
fn tab_widget(ui: &mut egui::Ui, label: &str, selected: bool) -> (egui::Response, bool) {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let pad = 8.0;
    let x_size = 18.0;
    let text_max = (TAB_MAX_WIDTH - 2.0 * pad - x_size).max(24.0);

    // Lay the label out on a single line, ellipsizing if it does not fit.
    let mut job = egui::text::LayoutJob::single_section(
        label.to_owned(),
        egui::TextFormat {
            font_id: font_id.clone(),
            color: ui.visuals().text_color(),
            ..Default::default()
        },
    );
    job.wrap.max_width = text_max;
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);

    let width = (galley.size().x + 2.0 * pad + x_size).min(TAB_MAX_WIDTH);
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::click_and_drag());

    let visuals = ui.style().visuals.clone();
    // The selected tab matches the content below so it reads as connected.
    let fill = if selected {
        visuals.panel_fill
    } else {
        egui::Color32::from_gray(178)
    };
    let border = egui::Color32::from_gray(120);
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, fill);
    let stroke = egui::Stroke::new(1.0, border);
    painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
    painter.line_segment([rect.left_top(), rect.right_top()], stroke);
    painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
    let text_pos = egui::pos2(rect.left() + pad, rect.center().y - galley.size().y * 0.5);
    painter.galley(text_pos, galley, visuals.text_color());

    // Close button at the right of the label.
    let x_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - x_size * 0.5 - 1.0, rect.center().y),
        egui::vec2(x_size, x_size),
    );
    let x_resp = ui.interact(x_rect, resp.id.with("close"), egui::Sense::click());
    let x_color = if x_resp.hovered() {
        visuals.strong_text_color()
    } else {
        visuals.weak_text_color()
    };
    ui.painter().text(
        x_rect.center(),
        egui::Align2::CENTER_CENTER,
        "×",
        font_id,
        x_color,
    );

    (resp.on_hover_text(label.to_owned()), x_resp.clicked())
}

impl eframe::App for Workspace {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::T)) {
            let shared = self.shared.clone();
            self.windows[0].new_tab(ctx, self.has_gpu, self.srgb_output, &shared);
        }

        // Main window.
        let result = Workspace::render_window(
            ctx,
            &mut self.windows[0],
            &mut self.shared,
            &mut self.drag,
            self.has_gpu,
            self.srgb_output,
        );
        if result.closed {
            // The main window closed (its last tab was closed): exit the app.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if let Some((index, pos)) = result.tear {
            self.tear_off(0, index, pos);
        }

        // Torn-off windows.
        let mut index = 1;
        while index < self.windows.len() {
            if self.render_torn(ctx, index) {
                // The window closed; the next one shifted into this slot.
            } else {
                index += 1;
            }
        }

        // Resolve a cross-window drag once every window has been drawn.
        self.resolve_drag();
    }
}

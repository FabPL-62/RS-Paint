//! The multi-document workspace: a list of tabs (one editor each) plus the
//! cross-tab shared state, drag-to-reorder, drag-between-windows and tear-off.

use eframe::egui;

use crate::app::{PaintApp, Shared};
use crate::ui::tabs::{tab_widget, TAB_HEIGHT};

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
        tab.editor.apply_shared(shared);
        tab.editor.invalidate_canvas();
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let closing_active = index == self.active;
        self.tabs.remove(index);
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        if closing_active {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                // Canvas GPU resources are shared by the tabs in this window.
                // Restore the newly visible document just as on a tab switch.
                tab.editor.invalidate_canvas();
            }
        }
    }

    fn open_pending_document(
        &mut self,
        ctx: &egui::Context,
        has_gpu: bool,
        srgb: bool,
        shared: &Shared,
    ) {
        if let Some((buffer, path)) = self.tabs[self.active].take_opened_document() {
            let reuse = self.tabs[self.active].editor.is_pristine_new_document();
            if !reuse {
                self.new_tab(ctx, has_gpu, srgb, shared);
            }
            self.tabs[self.active]
                .editor
                .load_buffer(buffer, Some(path));
            ctx.request_repaint();
        }
    }

    fn open_dropped_images(
        &mut self,
        ctx: &egui::Context,
        has_gpu: bool,
        srgb: bool,
        shared: &Shared,
    ) {
        let files = ctx.input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
        if files.is_empty() {
            return;
        }
        for file in files {
            if let Some(path) = file.path.filter(|path| PaintApp::supports_image_path(path)) {
                self.tabs[self.active].open_image_path(ctx, &path);
                self.open_pending_document(ctx, has_gpu, srgb, shared);
            }
        }
        ctx.request_repaint();
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
        let shared = first.editor.capture_shared();
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
            let modified = window.tabs.iter().any(|tab| tab.editor.is_modified());
            if modified && !window.force_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                window.confirm_close = true;
            }
        }

        // Apply the action requested by the tab bar on the previous frame.
        let mut tear = None;
        match std::mem::replace(&mut window.pending, TabAction::None) {
            TabAction::Switch(index) if index < window.tabs.len() => {
                *shared = window.tabs[window.active].editor.capture_shared();
                window.active = index;
                window.tabs[index].editor.invalidate_canvas();
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
        window.tabs[active].editor.apply_shared(shared);
        window.tabs[active].update_editor_top(ctx);
        // The tab strip sits below the menu, right of the tool box and above
        // the canvas: it is drawn between the two editor halves.
        show_tab_bar(ctx, window, drag);
        window.tabs[active].update_editor_bottom(ctx);
        show_window_close_confirm(ctx, window);
        *shared = window.tabs[active].editor.capture_shared();
        window.open_pending_document(ctx, has_gpu, srgb, shared);
        window.open_dropped_images(ctx, has_gpu, srgb, shared);
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
        tab.editor.invalidate_canvas();
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
            .map(|tab| tab.tab_label(ctx))
            .unwrap_or_default();
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("{title} - RS Paint"))
            .with_inner_size([900.0, 700.0])
            .with_drag_and_drop(true)
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
                tab.editor.invalidate_canvas();
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
    let strip_fill = crate::theme::STRIP;
    egui::TopBottomPanel::top(panel_id)
        .frame(
            egui::Frame::new()
                .fill(strip_fill)
                .inner_margin(egui::Margin {
                    left: 6,
                    right: 6,
                    top: 0,
                    bottom: 0,
                }),
        )
        .show(ctx, |ui| {
            let strip_bottom = ui.cursor().top() + TAB_HEIGHT;
            let mut rects: Vec<egui::Rect> = Vec::new();
            egui::ScrollArea::horizontal()
                .id_salt(("tabs-scroll", window.id))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for index in 0..window.tabs.len() {
                            let label = window.tabs[index].tab_label(ctx);
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
                                                    window.pending =
                                                        TabAction::TearOff(d.index, pos);
                                                } else {
                                                    // Reorder by the position under the
                                                    // pointer (count the tab centers to
                                                    // its left).
                                                    let from = d.index;
                                                    let target = rects
                                                        .iter()
                                                        .filter(|r| r.center().x < pos.x)
                                                        .count();
                                                    let to = if target > from {
                                                        target - 1
                                                    } else {
                                                        target
                                                    };
                                                    if to != from {
                                                        window.pending =
                                                            TabAction::Reorder(from, to);
                                                    }
                                                }
                                            }
                                        }
                                        *drag = None;
                                    }
                                }
                            }
                        }
                        if ui
                            .add_sized(
                                egui::vec2(TAB_HEIGHT, TAB_HEIGHT),
                                egui::Button::new("+")
                                    .corner_radius(0)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .on_hover_text(crate::i18n::tr(ui.ctx(), "New tab"))
                            .clicked()
                        {
                            window.pending = TabAction::New;
                        }
                    });
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
                .fill(crate::theme::PANEL)
                .stroke(egui::Stroke::new(1.0, crate::theme::ACCENT))
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
    egui::Window::new(crate::i18n::tr(ctx, "Close window"))
        .id(egui::Id::new("dialog-close-window"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.label(crate::i18n::tr(
                ui.ctx(),
                "There are unsaved changes in this window.",
            ));
            ui.label(crate::i18n::tr(
                ui.ctx(),
                "Are you sure you want to close it?",
            ));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr(ui.ctx(), "Cancel")).clicked() {
                    action = 1;
                }
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "Save and close"))
                    .clicked()
                {
                    action = 2;
                }
                if ui
                    .button(crate::i18n::tr(ui.ctx(), "Close without saving"))
                    .clicked()
                {
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
                if tab.editor.is_modified() {
                    tab.save(ctx);
                    if tab.editor.is_modified() {
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

/// Distance below the strip required to detach a dragged tab.
const TEAR_OFF_DISTANCE: f32 = 20.0;

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

#[cfg(test)]
mod tests {
    use super::*;
    use rspaint_core::{Color, PixelBuffer};
    use rspaint_editor::PointerButton;

    #[test]
    fn closing_active_tab_restores_the_surviving_document_texture() {
        let ctx = egui::Context::default();
        for active in 0..3 {
            let original = PaintApp::new_blank(true, false, &ctx);
            let shared = original.editor.capture_shared();
            let mut window = WindowState::new(egui::ViewportId::ROOT, 0, original);
            window.new_tab(&ctx, true, false, &shared);
            window.new_tab(&ctx, true, false, &shared);
            for (index, tab) in window.tabs.iter_mut().enumerate() {
                tab.editor.load_buffer(
                    PixelBuffer::new(8 + index as u32, 6, Color::rgb(index as u8, 80, 150)),
                    None,
                );
                assert!(tab.editor.take_pending_upload().is_some());
                assert!(tab.editor.take_pending_upload().is_none());
            }
            window.active = active;
            window.close_tab(active);
            let editor = &mut window.tabs[window.active].editor;
            let upload = editor
                .take_pending_upload()
                .expect("visible tab must restore its texture");
            assert_eq!((upload.x, upload.y), (0, 0));
            assert_eq!(
                (upload.width, upload.height),
                (editor.document.width(), editor.document.height())
            );
            assert_eq!(upload.data.as_slice(), editor.document.buffer.data());
            // Closing an inactive tab keeps the same document visible.
            window.close_tab(1 - window.active);
            assert!(window.tabs[window.active]
                .editor
                .take_pending_upload()
                .is_none());
            window.close_tab(0);
            assert!(window.tabs.is_empty());
        }
    }

    struct DropFiles(std::path::PathBuf);

    impl DropFiles {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory =
                std::env::temp_dir().join(format!("rspaint-drop-{}-{unique}", std::process::id()));
            std::fs::create_dir(&directory).unwrap();
            let pixels = PixelBuffer::new(12, 8, Color::rgb(20, 80, 150));
            std::fs::write(
                directory.join("first.PNG"),
                rspaint_core::io::encode(&pixels, rspaint_core::io::SaveFormat::Png).unwrap(),
            )
            .unwrap();
            std::fs::write(
                directory.join("second.bmp"),
                rspaint_core::io::encode(&pixels, rspaint_core::io::SaveFormat::Bmp).unwrap(),
            )
            .unwrap();
            std::fs::write(directory.join("unsupported.txt"), b"not an image").unwrap();
            std::fs::write(directory.join("broken.png"), b"not an image").unwrap();
            Self(directory)
        }
        fn input(&self, names: &[&str]) -> egui::RawInput {
            egui::RawInput {
                dropped_files: names
                    .iter()
                    .map(|name| egui::DroppedFile {
                        path: Some(self.0.join(name)),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }
        }
    }
    impl Drop for DropFiles {
        fn drop(&mut self) {
            for name in ["first.PNG", "second.bmp", "unsupported.txt", "broken.png"] {
                let _ = std::fs::remove_file(self.0.join(name));
            }
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn dropping_multiple_images_reuses_blank_then_opens_each_in_its_own_tab() {
        let files = DropFiles::new();
        let ctx = egui::Context::default();
        let original = PaintApp::new_blank(true, false, &ctx);
        let shared = original.editor.capture_shared();
        let mut window = WindowState::new(egui::ViewportId::ROOT, 0, original);
        let _ = ctx.run(
            files.input(&["first.PNG", "unsupported.txt", "second.bmp"]),
            |ctx| {
                window.open_dropped_images(ctx, true, false, &shared);
                // Another layout pass cannot reopen consumed files.
                window.open_dropped_images(ctx, true, false, &shared);
            },
        );
        assert_eq!(window.tabs.len(), 2);
        assert_eq!(window.active, 1);
        assert_eq!(window.tabs[0].editor.document.name, "first");
        assert_eq!(window.tabs[1].editor.document.name, "second");
        for tab in &window.tabs {
            assert_eq!(
                tab.editor.document.buffer.get(0, 0),
                Some(Color::rgb(20, 80, 150))
            );
            assert!(!tab.editor.is_modified());
        }
    }

    #[test]
    fn dropped_image_preserves_edited_document_and_corrupt_file_creates_no_tab() {
        let files = DropFiles::new();
        let ctx = egui::Context::default();
        let mut original = PaintApp::new_blank(true, false, &ctx);
        original.editor.begin_stroke((3, 3), PointerButton::Primary);
        original.editor.end_stroke(false);
        let pixels = original.editor.document.buffer.clone();
        let shared = original.editor.capture_shared();
        let mut window =
            WindowState::new(egui::ViewportId::from_hash_of("detached-test"), 1, original);
        let _ = ctx.run(files.input(&["first.PNG", "broken.png"]), |ctx| {
            window.open_dropped_images(ctx, true, false, &shared);
        });
        assert_eq!(window.tabs.len(), 2);
        assert_eq!(window.tabs[0].editor.document.buffer, pixels);
        assert!(window.tabs[0].editor.is_modified());
        assert_eq!(window.tabs[1].editor.document.name, "first");
        assert!(window.tabs[1].error_message.is_some());
    }

    #[test]
    fn first_open_reuses_the_untouched_blank_document() {
        let ctx = egui::Context::default();
        let original = PaintApp::new_blank(true, false, &ctx);
        let shared = original.editor.capture_shared();
        let mut window = WindowState::new(egui::ViewportId::ROOT, 0, original);
        let opened = PixelBuffer::new(12, 8, Color::WHITE);
        window.tabs[0].request_open_document(opened.clone(), "first.png".into());
        window.open_pending_document(&ctx, true, false, &shared);
        assert_eq!(window.tabs.len(), 1);
        assert_eq!(window.active, 0);
        assert_eq!(window.tabs[0].editor.document.buffer, opened);
        assert_eq!(window.tabs[0].editor.document.name, "first");
        window.tabs[0]
            .request_open_document(PixelBuffer::new(4, 4, Color::BLACK), "second.png".into());
        window.open_pending_document(&ctx, true, false, &shared);
        assert_eq!(window.tabs.len(), 2);
        assert_eq!(window.active, 1);
        assert_eq!(window.tabs[0].editor.document.buffer, opened);
    }

    #[test]
    fn undoing_back_to_white_does_not_make_the_startup_tab_disposable() {
        let ctx = egui::Context::default();
        let mut original = PaintApp::new_blank(true, false, &ctx);
        original.editor.begin_stroke((3, 3), PointerButton::Primary);
        original.editor.end_stroke(false);
        original.editor.undo();
        assert_eq!(
            original.editor.document.buffer.get(3, 3),
            Some(Color::WHITE)
        );
        let shared = original.editor.capture_shared();
        let mut window = WindowState::new(egui::ViewportId::ROOT, 0, original);
        window.tabs[0]
            .request_open_document(PixelBuffer::new(4, 4, Color::BLACK), "opened.png".into());
        window.open_pending_document(&ctx, true, false, &shared);
        assert_eq!(window.tabs.len(), 2);
        assert!(window.tabs[0].editor.history.can_redo());
    }

    #[test]
    fn opening_an_image_adds_a_tab_and_preserves_the_unsaved_document() {
        let ctx = egui::Context::default();
        let mut original = PaintApp::new_blank(true, false, &ctx);
        original.editor.begin_stroke((3, 3), PointerButton::Primary);
        original.editor.end_stroke(false);
        original.editor.select_all();
        let pixels = original.editor.document.buffer.clone();
        let history = original.editor.history.current();
        let shared = original.editor.capture_shared();
        let mut window = WindowState::new(egui::ViewportId::ROOT, 0, original);
        let opened = PixelBuffer::new(12, 8, Color::rgb(20, 80, 150));
        window.tabs[0].request_open_document(opened.clone(), "landscape.png".into());
        window.open_pending_document(&ctx, true, false, &shared);
        assert_eq!(window.tabs.len(), 2);
        assert_eq!(window.active, 1);
        assert_eq!(window.tabs[0].editor.document.buffer, pixels);
        assert_eq!(window.tabs[0].editor.history.current(), history);
        assert!(window.tabs[0].editor.is_modified());
        assert!(window.tabs[0].editor.selection.is_some());
        assert_eq!(window.tabs[1].editor.document.buffer, opened);
        assert_eq!(window.tabs[1].editor.document.name, "landscape");
        assert_eq!(
            window.tabs[1].editor.file_path.as_deref(),
            Some(std::path::Path::new("landscape.png"))
        );
        assert!(!window.tabs[1].editor.is_modified());
        // A request is handled only once, without adding extra tabs next frame.
        window.open_pending_document(&ctx, true, false, &shared);
        assert_eq!(window.tabs.len(), 2);
    }
}

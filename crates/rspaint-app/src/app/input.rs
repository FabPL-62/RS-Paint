use super::*;

impl PaintApp {
    pub(super) fn update_pick_hold(&mut self, ctx: &egui::Context) {
        let (ctrl, alt) = ctx.input(|i| (i.modifiers.command, i.modifiers.alt));
        self.editor.pick_hold = if !self.editor.settings.tool.allows_color_pick() {
            None
        } else if alt {
            Some(PickTarget::Secondary)
        } else if ctrl {
            Some(PickTarget::Primary)
        } else {
            None
        };
    }

    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
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
            if self.editor.has_text_box() {
                self.ctrl_down = i.modifiers.command;
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.editor.commit_text();
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
            let cut_cmd = i.events.iter().any(|event| {
                matches!(event, egui::Event::Cut)
                    || matches!(
                        event,
                        egui::Event::Key { key: Key::X, pressed: false, modifiers, .. }
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
                self.editor.redo();
            } else if i.consume_key(Modifiers::CTRL, Key::Z) {
                self.editor.undo();
            } else if i.consume_key(ctrl_shift, Key::S) {
                action = Some(FileAction::SaveAs);
            } else if i.consume_key(Modifiers::CTRL, Key::S) {
                action = Some(FileAction::Save);
            } else if i.consume_key(Modifiers::CTRL, Key::O) {
                action = Some(FileAction::Open);
            } else if i.consume_key(ctrl_shift, Key::Y) {
                self.show_history = true;
            } else if i.consume_key(Modifiers::CTRL, Key::I) {
                self.editor.invert_colors();
            } else if i.consume_key(Modifiers::CTRL, Key::E) {
                self.editor.select_all();
                // With the Outliner, Ctrl+E applies it to the whole image.
                if self.editor.settings.tool == Tool::Outliner {
                    self.editor.apply_outliner();
                    self.editor.deselect();
                }
            } else if i.consume_key(Modifiers::CTRL, Key::R) {
                self.show_flip_rotate = true;
            } else if i.consume_key(Modifiers::CTRL, Key::W) {
                self.show_stretch_skew = true;
            } else if i.consume_key(Modifiers::CTRL, Key::G) {
                self.editor.toggle_grid();
            } else if i.consume_key(Modifiers::CTRL, Key::F) {
                self.show_bitmap = !self.show_bitmap;
            } else if cut_cmd {
                self.cut_selection();
            } else if copy_cmd {
                self.copy_selection();
            } else if paste_cmd {
                self.paste_clipboard();
            } else if i.consume_key(Modifiers::NONE, Key::Delete)
                || i.consume_key(Modifiers::NONE, Key::Backspace)
            {
                self.editor.delete_selection();
            } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                if self.editor.has_text_box() {
                    self.editor.commit_text();
                } else if !self.editor.curve_points().is_empty() {
                    self.editor.curve_cancel();
                } else if self.editor.polygon_points().is_empty() {
                    self.editor.deselect();
                } else {
                    self.editor.cancel_polygon();
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
                self.editor.set_tool(tool);
            }
        }
        match action {
            Some(FileAction::Save) => self.save(ctx),
            Some(FileAction::SaveAs) => self.save_as(ctx),
            Some(FileAction::Open) => self.open_file_dialog(ctx),
            None => {}
        }
        // With a selection tool, Ctrl is used to reveal the transform controls,
        // so a bare tap must not also toggle the selection transparency.
        if ctrl_tap && !self.editor.settings.tool.is_selection() {
            self.editor.toggle_selection_transparency();
        }
        if size_delta != 0 {
            self.editor.adjust_tool_size(size_delta);
        }
        if radius_delta != 0 && self.editor.settings.tool == Tool::RoundedRectangle {
            self.editor.adjust_round_radius(radius_delta);
        }
    }
}

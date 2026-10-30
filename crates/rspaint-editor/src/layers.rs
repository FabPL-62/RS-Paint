//! Layer stack operations and the active-layer rules.

use super::*;

impl Editor {
    /// The layer tree, topmost first.
    pub fn layers(&self) -> &[LayerNode] {
        &self.document.layers
    }

    /// The active node (the drawing target when it is a layer).
    pub fn active_layer_id(&self) -> u64 {
        self.document.active
    }

    pub fn active_node(&self) -> Option<&LayerNode> {
        rspaint_core::layer::find_node(&self.document.layers, self.document.active)
    }

    pub fn layer_node(&self, id: u64) -> Option<&LayerNode> {
        rspaint_core::layer::find_node(&self.document.layers, id)
    }

    /// The ids selected in the layer panel.
    pub fn layer_selection(&self) -> &[u64] {
        &self.layer_selection
    }

    /// The visible rows of the panel, topmost first.
    pub fn layer_rows(&self) -> Vec<LayerRow<'_>> {
        rspaint_core::layer::visible_rows(&self.document.layers)
    }

    /// A layer's pixels. The active layer is read from its working buffer.
    pub fn layer_pixels(&self, id: u64) -> Option<&PixelBuffer> {
        if id == self.document.active && matches!(self.active_node(), Some(LayerNode::Layer(_))) {
            return Some(&self.document.buffer);
        }
        self.layer_node(id)
            .and_then(LayerNode::layer)
            .map(|layer| &layer.pixels)
    }

    /// A change counter for thumbnails, unique per layer content.
    pub fn layer_revision(&self, id: u64) -> u64 {
        if id == self.document.active {
            self.document.active_revision
        } else {
            self.layer_node(id).map(LayerNode::revision).unwrap_or(0)
        }
    }

    /// Whether the active node exists, is a layer and is not hidden or locked
    /// (nor inside a hidden or locked group).
    pub fn can_paint_active(&self) -> bool {
        let mut chain = Vec::new();
        if !node_chain(&self.document.layers, self.document.active, &mut chain) {
            return false;
        }
        if !matches!(chain.last(), Some(LayerNode::Layer(_))) {
            return false;
        }
        chain.iter().all(|node| node.visible() && !node.locked())
    }

    /// Handles a click on a layer row.
    ///
    /// A plain click makes the row active. Ctrl toggles it in the selection and
    /// Shift extends from the anchor; neither changes the drawing target.
    pub fn select_layer(&mut self, id: u64, shift: bool, ctrl: bool) {
        if rspaint_core::layer::find_node(&self.document.layers, id).is_none() {
            return;
        }
        if !shift && !ctrl {
            if id != self.document.active {
                self.settle_selection();
                self.commit_text();
                self.selection_undo.clear();
                self.document.activate(id);
            }
            self.layer_selection = vec![id];
            self.layer_anchor = Some(id);
            return;
        }

        if ctrl {
            if let Some(index) = self
                .layer_selection
                .iter()
                .position(|&selected| selected == id)
            {
                if self.layer_selection.len() > 1 {
                    self.layer_selection.remove(index);
                }
            } else {
                self.layer_selection.push(id);
            }
            return;
        }

        // Shift: select every visible row between the anchor and this one.
        let anchor = self
            .layer_anchor
            .filter(|&anchor| {
                rspaint_core::layer::find_node(&self.document.layers, anchor).is_some()
            })
            .unwrap_or(self.document.active);
        let rows: Vec<u64> = self.layer_rows().iter().map(|row| row.node.id()).collect();
        let (Some(a), Some(b)) = (
            rows.iter().position(|&row| row == anchor),
            rows.iter().position(|&row| row == id),
        ) else {
            self.layer_selection = vec![id];
            return;
        };
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        self.layer_selection = rows[start..=end].to_vec();
    }

    /// Adds a transparent layer above the active row and makes it active.
    pub fn add_layer(&mut self) {
        self.settle_selection();
        self.commit_text();
        self.document.sync_active();
        let (parent, index) = insertion_point(&self.document.layers, self.document.active);
        let id = self.document.next_id;
        self.document.next_id += 1;
        let name = format!("Layer {}", self.document.layer_counter);
        self.document.layer_counter += 1;
        let pixels = PixelBuffer::transparent(self.document.width(), self.document.height());
        rspaint_core::layer::insert_node(
            &mut self.document.layers,
            parent,
            index,
            LayerNode::Layer(rspaint_core::Layer::new(id, name, pixels)),
        );
        self.activate_new_node(id);
        self.mark_full();
        self.commit("Add Layer");
    }

    /// Adds an empty group folder above the active row and makes it active.
    pub fn add_group(&mut self) {
        self.settle_selection();
        self.commit_text();
        self.document.sync_active();
        let (parent, index) = insertion_point(&self.document.layers, self.document.active);
        let id = self.document.next_id;
        self.document.next_id += 1;
        let name = format!("Group {}", self.document.group_counter);
        self.document.group_counter += 1;
        rspaint_core::layer::insert_node(
            &mut self.document.layers,
            parent,
            index,
            LayerNode::Group(rspaint_core::LayerGroup::new(id, name)),
        );
        self.activate_new_node(id);
        self.mark_full();
        self.commit("Add Group");
    }

    fn activate_new_node(&mut self, id: u64) {
        self.document.active = id;
        self.document.refresh_buffer();
        self.layer_selection = vec![id];
        self.layer_anchor = Some(id);
        self.expand_ancestors(id);
    }

    fn expand_ancestors(&mut self, id: u64) {
        let mut chain = Vec::new();
        if node_chain(&self.document.layers, id, &mut chain) {
            let ids: Vec<u64> = chain.iter().map(|node| node.id()).collect();
            for id in ids {
                if let Some(node) =
                    rspaint_core::layer::find_node_mut(&mut self.document.layers, id)
                {
                    node.set_expanded(true);
                }
            }
        }
    }

    /// Deletes the selected rows. The last remaining node is never removed.
    pub fn delete_selected_layers(&mut self) {
        let mut ids = self.selected_ancestors();
        if ids.is_empty() {
            ids.push(self.document.active);
        }
        ids.retain(|&id| rspaint_core::layer::find_node(&self.document.layers, id).is_some());
        if ids.is_empty() {
            return;
        }
        let total = count_nodes(&self.document.layers);
        let removed: usize = ids
            .iter()
            .map(|&id| {
                self.layer_node(id)
                    .map(|node| count_nodes(std::slice::from_ref(node)).max(1))
                    .unwrap_or(0)
            })
            .sum();
        if removed >= total {
            return;
        }
        self.settle_selection();
        self.commit_text();
        self.document.sync_active();

        let rows: Vec<u64> = self.layer_rows().iter().map(|row| row.node.id()).collect();
        let top = rows.iter().position(|id| ids.contains(id)).unwrap_or(0);
        for id in &ids {
            rspaint_core::layer::remove_node(&mut self.document.layers, *id);
        }
        let remaining: Vec<u64> = self.layer_rows().iter().map(|row| row.node.id()).collect();
        let new_active = remaining
            .get(top.min(remaining.len().saturating_sub(1)))
            .or_else(|| remaining.last())
            .copied();
        let active_deleted = ids.contains(&self.document.active)
            || rspaint_core::layer::find_node(&self.document.layers, self.document.active)
                .is_none();
        if active_deleted {
            if let Some(id) = new_active {
                self.document.active = id;
                self.document.refresh_buffer();
            }
        }
        self.layer_selection = vec![self.document.active];
        self.layer_anchor = Some(self.document.active);
        self.mark_full();
        self.commit("Delete Layer");
    }

    /// Whether the delete action can remove the current selection.
    pub fn can_delete_layers(&self) -> bool {
        let mut ids = self.selected_ancestors();
        if ids.is_empty() {
            ids.push(self.document.active);
        }
        let total = count_nodes(&self.document.layers);
        let removed: usize = ids
            .iter()
            .map(|&id| {
                self.layer_node(id)
                    .map(|node| count_nodes(std::slice::from_ref(node)).max(1))
                    .unwrap_or(0)
            })
            .sum();
        removed > 0 && removed < total
    }

    /// Merges the selected rows into one layer at the topmost selected spot.
    pub fn merge_selected_layers(&mut self) {
        let mut ids = self.selected_ancestors();
        if ids.len() < 2 {
            return;
        }
        let rows: Vec<(u64, Option<u64>, usize)> = self
            .layer_rows()
            .iter()
            .filter_map(|row| {
                let id = row.node.id();
                ids.contains(&id).then(|| {
                    let (parent, index) =
                        rspaint_core::layer::parent_and_index(&self.document.layers, id)
                            .unwrap_or((row.parent, 0));
                    (id, parent, index)
                })
            })
            .collect();
        let Some(&(_, parent, index)) = rows.first() else {
            return;
        };
        self.settle_selection();
        self.commit_text();
        self.document.sync_active();
        ids.retain(|id| rspaint_core::layer::find_node(&self.document.layers, *id).is_some());
        let selected: Vec<LayerNode> = ids
            .iter()
            .filter_map(|id| rspaint_core::layer::find_node(&self.document.layers, *id).cloned())
            .collect();
        if selected.len() < 2 {
            return;
        }
        let (width, height) = (self.document.width(), self.document.height());
        let mut merged = PixelBuffer::transparent(width, height);
        rspaint_core::layer::composite_region(
            &selected,
            self.document.active,
            &self.document.buffer,
            rspaint_core::layer::Region::new(0, 0, width, height),
            &mut merged,
        );
        let top_name = selected
            .first()
            .map(|node| node.name().to_owned())
            .unwrap_or_else(|| "Merged".to_owned());
        let new_id = self.document.next_id;
        self.document.next_id += 1;
        for id in &ids {
            rspaint_core::layer::remove_node(&mut self.document.layers, *id);
        }
        rspaint_core::layer::insert_node(
            &mut self.document.layers,
            parent,
            index,
            LayerNode::Layer(rspaint_core::Layer::new(new_id, top_name, merged)),
        );
        self.document.active = new_id;
        self.document.refresh_buffer();
        self.layer_selection = vec![new_id];
        self.layer_anchor = Some(new_id);
        self.mark_full();
        self.commit("Merge Layers");
    }

    /// Renames a layer or group.
    pub fn rename_layer(&mut self, id: u64, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let Some(node) = rspaint_core::layer::find_node_mut(&mut self.document.layers, id) else {
            return;
        };
        if node.name() == name {
            return;
        }
        node.set_name(name);
        self.commit("Rename Layer");
    }

    pub fn toggle_layer_visible(&mut self, id: u64) {
        let Some(node) = rspaint_core::layer::find_node_mut(&mut self.document.layers, id) else {
            return;
        };
        let visible = !node.visible();
        node.set_visible(visible);
        self.mark_full();
        self.commit("Layer Visibility");
    }

    pub fn toggle_layer_locked(&mut self, id: u64) {
        let Some(node) = rspaint_core::layer::find_node_mut(&mut self.document.layers, id) else {
            return;
        };
        let locked = !node.locked();
        node.set_locked(locked);
    }

    /// Applies an opacity change without committing (the panel commits on
    /// release, so dragging the slider is a single undo step).
    pub fn set_layer_opacity(&mut self, id: u64, opacity: u8) {
        let Some(node) = rspaint_core::layer::find_node_mut(&mut self.document.layers, id) else {
            return;
        };
        if node.opacity() != opacity.min(100) {
            node.set_opacity(opacity);
            self.mark_full();
        }
    }

    pub fn set_layer_blend(&mut self, id: u64, blend: LayerBlend) {
        let Some(node) = rspaint_core::layer::find_node_mut(&mut self.document.layers, id) else {
            return;
        };
        if node.blend() != blend {
            node.set_blend(blend);
            self.mark_full();
            self.commit("Layer Blend");
        }
    }

    pub fn toggle_group_expanded(&mut self, id: u64) {
        let Some(node) = rspaint_core::layer::find_node_mut(&mut self.document.layers, id) else {
            return;
        };
        let expanded = !node.expanded();
        node.set_expanded(expanded);
    }

    /// Moves `id` before `target` in the target's parent.
    pub fn move_layer_before(&mut self, id: u64, target: u64) {
        let Some((parent, index)) =
            rspaint_core::layer::parent_and_index(&self.document.layers, target)
        else {
            return;
        };
        self.relocate_layer(id, parent, index);
    }

    /// Moves `id` after `target` in the target's parent.
    pub fn move_layer_after(&mut self, id: u64, target: u64) {
        let Some((parent, index)) =
            rspaint_core::layer::parent_and_index(&self.document.layers, target)
        else {
            return;
        };
        self.relocate_layer(id, parent, index + 1);
    }

    /// Moves `id` into the front of a group folder.
    pub fn move_layer_into(&mut self, id: u64, group: u64) {
        if self.layer_node(group).is_none_or(|node| !node.is_group()) {
            return;
        }
        self.relocate_layer(id, Some(group), 0);
    }

    /// Moves `id` to the front of the root list.
    pub fn move_layer_to_start(&mut self, id: u64) {
        self.relocate_layer(id, None, 0);
    }

    /// Moves `id` to the back of the root list.
    pub fn move_layer_to_end(&mut self, id: u64) {
        let end = self.document.layers.len();
        self.relocate_layer(id, None, end);
    }

    fn relocate_layer(&mut self, id: u64, parent: Option<u64>, mut index: usize) {
        if parent == Some(id) {
            return;
        }
        if let Some(parent) = parent {
            if rspaint_core::layer::is_ancestor(&self.document.layers, id, parent) {
                return;
            }
        }
        let Some((current_parent, current_index)) =
            rspaint_core::layer::parent_and_index(&self.document.layers, id)
        else {
            return;
        };
        if current_parent == parent && current_index < index {
            index -= 1;
        }
        self.document.sync_active();
        let Some(node) = rspaint_core::layer::remove_node(&mut self.document.layers, id) else {
            return;
        };
        rspaint_core::layer::insert_node(&mut self.document.layers, parent, index, node);
        self.mark_full();
        self.commit("Move Layer");
    }

    /// The selected ids with descendants of other selected nodes removed.
    fn selected_ancestors(&self) -> Vec<u64> {
        let ids: Vec<u64> = self.layer_selection.clone();
        ids.iter()
            .copied()
            .filter(|&id| {
                rspaint_core::layer::find_node(&self.document.layers, id).is_some()
                    && !ids.iter().any(|&other| {
                        other != id
                            && rspaint_core::layer::is_ancestor(&self.document.layers, other, id)
                    })
            })
            .collect()
    }
}

/// Finds the insertion point for a new node: inside an active group at the
/// front, otherwise right above the active row.
fn insertion_point(nodes: &[LayerNode], active: u64) -> (Option<u64>, usize) {
    match rspaint_core::layer::find_node(nodes, active) {
        Some(node) if node.is_group() => (Some(active), 0),
        Some(_) => rspaint_core::layer::parent_and_index(nodes, active).unwrap_or((None, 0)),
        None => (None, 0),
    }
}

fn node_chain<'a>(nodes: &'a [LayerNode], id: u64, chain: &mut Vec<&'a LayerNode>) -> bool {
    for node in nodes {
        chain.push(node);
        if node.id() == id {
            return true;
        }
        if node_chain(node.children(), id, chain) {
            return true;
        }
        chain.pop();
    }
    false
}

fn count_nodes(nodes: &[LayerNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + count_nodes(node.children()))
        .sum()
}

//! Non-linear undo/redo history.
//!
//! Mirrors JS Paint's history *tree*: undoing and then doing something new
//! creates a new branch instead of discarding the previous redo states. Each
//! node stores a full image snapshot (region deltas are a future optimization).

use crate::buffer::PixelBuffer;

/// Identifier of a node within a [`History`].
pub type NodeId = usize;

/// A single state in the history tree.
#[derive(Clone, Debug)]
pub struct HistoryNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Human-readable action name, e.g. `"Pencil"`.
    pub name: String,
    /// The image state *after* this action.
    pub snapshot: PixelBuffer,
}

/// A branching history of image states.
#[derive(Clone, Debug)]
pub struct History {
    nodes: Vec<HistoryNode>,
    current: NodeId,
}

impl History {
    /// Creates a history with a single root state.
    pub fn new(initial: PixelBuffer, name: impl Into<String>) -> Self {
        let root = HistoryNode {
            id: 0,
            parent: None,
            children: Vec::new(),
            name: name.into(),
            snapshot: initial,
        };
        Self {
            nodes: vec![root],
            current: 0,
        }
    }

    /// The id of the current state.
    pub fn current(&self) -> NodeId {
        self.current
    }

    /// The image at the current state.
    pub fn current_snapshot(&self) -> &PixelBuffer {
        &self.nodes[self.current].snapshot
    }

    /// The name of the current action.
    pub fn current_name(&self) -> &str {
        &self.nodes[self.current].name
    }

    /// Looks up a node by id.
    pub fn node(&self, id: NodeId) -> Option<&HistoryNode> {
        self.nodes.get(id)
    }

    /// All nodes, in creation order.
    pub fn nodes(&self) -> &[HistoryNode] {
        &self.nodes
    }

    /// Records a new state as a child of the current one and makes it current.
    pub fn commit(&mut self, name: impl Into<String>, snapshot: PixelBuffer) -> NodeId {
        let id = self.nodes.len();
        let parent = self.current;
        self.nodes.push(HistoryNode {
            id,
            parent: Some(parent),
            children: Vec::new(),
            name: name.into(),
            snapshot,
        });
        self.nodes[parent].children.push(id);
        self.current = id;
        id
    }

    /// Whether an undo is possible.
    pub fn can_undo(&self) -> bool {
        self.nodes[self.current].parent.is_some()
    }

    /// Whether a redo is possible.
    pub fn can_redo(&self) -> bool {
        !self.nodes[self.current].children.is_empty()
    }

    /// Moves to the parent state. Returns `false` at the root.
    pub fn undo(&mut self) -> bool {
        match self.nodes[self.current].parent {
            Some(parent) => {
                self.current = parent;
                true
            }
            None => false,
        }
    }

    /// Moves to the most recently created child state. Returns `false` at a leaf.
    pub fn redo(&mut self) -> bool {
        match self.nodes[self.current].children.last() {
            Some(&child) => {
                self.current = child;
                true
            }
            None => false,
        }
    }

    /// Jumps to an arbitrary node. Returns `false` if the id is unknown.
    pub fn jump_to(&mut self, id: NodeId) -> bool {
        if id < self.nodes.len() {
            self.current = id;
            true
        } else {
            false
        }
    }

    /// The name of the action that [`Self::undo`] would reverse.
    pub fn undo_name(&self) -> Option<&str> {
        self.nodes[self.current]
            .parent
            .map(|parent| self.nodes[parent].name.as_str())
    }

    /// The name of the action that [`Self::redo`] would reapply.
    pub fn redo_name(&self) -> Option<&str> {
        self.nodes[self.current]
            .children
            .last()
            .map(|&child| self.nodes[child].name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    fn solid(color: Color) -> PixelBuffer {
        PixelBuffer::new(2, 2, color)
    }

    #[test]
    fn commit_and_undo_redo() {
        let mut history = History::new(solid(Color::WHITE), "New");
        assert!(!history.can_undo());
        assert!(!history.can_redo());

        history.commit("Pencil", solid(Color::BLACK));
        assert!(history.can_undo());
        assert_eq!(history.current_name(), "Pencil");
        assert_eq!(history.current_snapshot().get(0, 0), Some(Color::BLACK));

        assert!(history.undo());
        assert_eq!(history.current_snapshot().get(0, 0), Some(Color::WHITE));
        assert!(history.can_redo());

        assert!(history.redo());
        assert_eq!(history.current_snapshot().get(0, 0), Some(Color::BLACK));
    }

    #[test]
    fn branching_preserves_redos() {
        let mut history = History::new(solid(Color::WHITE), "New");
        let red = history.commit("Red", solid(Color::rgb(255, 0, 0)));
        let blue = history.commit("Blue", solid(Color::rgb(0, 0, 255)));
        history.undo(); // back to Red
        let green = history.commit("Green", solid(Color::rgb(0, 255, 0)));

        // Blue and Green both branch off Red instead of discarding Blue.
        let red_node = history.node(red).unwrap();
        assert_eq!(red_node.children, vec![blue, green]);
        assert_eq!(history.current(), green);
        assert_eq!(
            history.current_snapshot().get(0, 0),
            Some(Color::rgb(0, 255, 0))
        );
    }

    #[test]
    fn jump_to_arbitrary_node() {
        let mut history = History::new(solid(Color::WHITE), "New");
        let a = history.commit("A", solid(Color::BLACK));
        history.commit("B", solid(Color::rgb(1, 1, 1)));
        assert!(history.jump_to(a));
        assert_eq!(history.current(), a);
        assert!(!history.jump_to(999));
    }
}

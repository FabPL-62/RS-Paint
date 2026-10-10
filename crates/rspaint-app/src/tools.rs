//! Tool definitions and metadata.

/// The tools shown in the tool box, in MS Paint order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    FreeFormSelect,
    Select,
    MagicWand,
    SelectionBrush,
    Eraser,
    Fill,
    PickColor,
    Magnifier,
    Pencil,
    Brush,
    Airbrush,
    Text,
    Line,
    Curve,
    Rectangle,
    Polygon,
    Ellipse,
    RoundedRectangle,
    Outliner,
}

impl Tool {
    /// All tools, in tool box order.
    pub const ALL: [Tool; 19] = [
        Tool::FreeFormSelect,
        Tool::Select,
        Tool::MagicWand,
        Tool::SelectionBrush,
        Tool::Eraser,
        Tool::Fill,
        Tool::PickColor,
        Tool::Magnifier,
        Tool::Pencil,
        Tool::Brush,
        Tool::Airbrush,
        Tool::Text,
        Tool::Line,
        Tool::Curve,
        Tool::Rectangle,
        Tool::Polygon,
        Tool::Ellipse,
        Tool::RoundedRectangle,
        Tool::Outliner,
    ];

    /// The tool's display name (also used as its undo history entry).
    pub fn name(self) -> &'static str {
        match self {
            Tool::FreeFormSelect => "Free-Form Select",
            Tool::Select => "Select",
            Tool::MagicWand => "Magic Wand",
            Tool::SelectionBrush => "Selection Brush",
            Tool::Eraser => "Eraser",
            Tool::Fill => "Fill With Color",
            Tool::PickColor => "Pick Color",
            Tool::Magnifier => "Magnifier",
            Tool::Pencil => "Pencil",
            Tool::Brush => "Brush",
            Tool::Airbrush => "Airbrush",
            Tool::Text => "Text",
            Tool::Line => "Line",
            Tool::Curve => "Curve",
            Tool::Rectangle => "Rectangle",
            Tool::Polygon => "Polygon",
            Tool::Ellipse => "Ellipse",
            Tool::RoundedRectangle => "Rounded Rectangle",
            Tool::Outliner => "Outliner",
        }
    }

    /// The single-key shortcut that selects this tool, if any.
    pub fn shortcut(self) -> Option<char> {
        match self {
            Tool::Eraser => Some('E'),
            Tool::Fill => Some('F'),
            Tool::PickColor => Some('I'),
            Tool::Magnifier => Some('M'),
            Tool::Pencil => Some('P'),
            Tool::Brush => Some('B'),
            Tool::Airbrush => Some('A'),
            Tool::Text => Some('T'),
            Tool::Line => Some('L'),
            Tool::Curve => Some('C'),
            Tool::Rectangle => Some('R'),
            Tool::Ellipse => Some('O'),
            _ => None,
        }
    }

    /// Whether the tool is implemented in the current MVP.
    pub fn implemented(self) -> bool {
        true
    }

    /// Tools that draw continuously while the pointer is dragged.
    pub fn is_freehand(self) -> bool {
        matches!(
            self,
            Tool::Pencil | Tool::Brush | Tool::Airbrush | Tool::Eraser
        )
    }

    /// Tools defined by dragging from a start point to an end point.
    pub fn is_shape(self) -> bool {
        matches!(
            self,
            Tool::Line | Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle
        )
    }

    /// Tools that act on a single click instead of a drag.
    pub fn is_click_tool(self) -> bool {
        matches!(self, Tool::Fill | Tool::PickColor | Tool::Magnifier)
    }

    /// Selection tools (the Outliner also defines a rectangular selection).
    pub fn is_selection(self) -> bool {
        matches!(
            self,
            Tool::Select
                | Tool::FreeFormSelect
                | Tool::MagicWand
                | Tool::SelectionBrush
                | Tool::Outliner
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_have_unique_names() {
        let mut names: Vec<_> = Tool::ALL.iter().map(|t| t.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Tool::ALL.len());
    }

    #[test]
    fn tool_categories_are_disjoint() {
        for tool in Tool::ALL {
            let categories = [tool.is_freehand(), tool.is_shape(), tool.is_click_tool()]
                .iter()
                .filter(|x| **x)
                .count();
            assert!(categories <= 1, "{tool:?} is in multiple categories");
        }
    }
}

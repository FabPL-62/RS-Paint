//! Core logic for RS Paint, independent of any GUI.
//!
//! This crate is the Rust counterpart to the parts of JS Paint's `src/` that deal
//! with image state and editing, with all UI concerns stripped out. Keeping it
//! GUI-free makes it testable headlessly and reusable from a future WASM or CLI
//! front-end.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod brush;
pub mod buffer;
pub mod color;
mod composite_cache;
pub mod document;
pub mod draw;
pub mod fill;
pub mod history;
pub mod io;
pub mod layer;
pub mod palette;
mod parallel_composite;
pub mod selection;
pub mod snapshot;
pub mod transform;

pub use brush::{Brush, BrushShape};
pub use buffer::PixelBuffer;
pub use color::{BlendMode, Color};
pub use document::Document;
pub use history::{History, HistoryNode, NodeId};
pub use layer::{Layer, LayerBlend, LayerGroup, LayerNode, LayerRow, LayerStack};
pub use snapshot::HistorySnapshot;

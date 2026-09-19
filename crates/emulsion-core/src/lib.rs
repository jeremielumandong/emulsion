//! `emulsion-core` — the document model: a stack of nodes, the Command API
//! that is the only way to change it, and snapshot history.
//!
//! No GPU, no UI. Everything here runs headless and is tested headless.

pub mod command;
pub mod document;
pub mod geometry;
pub mod graph;
pub mod history;
pub mod node;

pub use command::{Command, CommandError, Dirty};
pub use document::{Document, DocumentError, PanelRow};
pub use history::{Editor, History};
pub use node::{Node, NodeId, NodeKind};

pub use emulsion_raster as raster;

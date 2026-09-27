//! `emulsion-core` — the document model: a stack of nodes, the Command API
//! that is the only way to change it, and snapshot history.
//!
//! No GPU, no UI. Everything here runs headless and is tested headless.

pub mod command;
pub mod creation;
pub mod design;
pub mod design_appearance;
pub mod design_charts;
pub mod design_layout;
pub mod design_metadata;
pub mod diagram;
pub mod document;
pub mod fragment;
pub mod geometry;
pub mod graph;
pub mod history;
pub mod layer_links;
pub mod node;
pub mod project;
pub mod raw;
pub mod smart;
pub mod styles;
pub mod text;
pub mod text_effects;
pub mod transform;
pub mod vector_cache;

pub use command::{Command, CommandError, Dirty};
pub use document::{Document, DocumentError, PanelRow};
pub use history::{Editor, History};
pub use node::{Node, NodeId, NodeKind};

pub use emulsion_raster as raster;

mod layer_locks;
mod layer_mask;

mod effect_render;
pub mod style_options;

#[cfg(test)]
mod styles_advanced_tests;

#[cfg(test)]
mod style_memory_tests;

mod composite_mask_cache;

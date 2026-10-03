//! `emulsion-core` — the document model: a stack of nodes, the Command API
//! that is the only way to change it, and snapshot history.
//!
//! No GPU, no UI. Everything here runs headless and is tested headless.

pub mod bucket;
pub mod command;
pub mod creation;
pub mod cutter;
pub mod design;
pub mod design_appearance;
pub mod design_background;
pub mod design_charts;
pub mod design_clipping;
pub mod design_components;
pub mod design_formatting;
pub mod design_interactions;
pub mod design_keyframes;
pub mod design_layout;
pub mod design_metadata;
pub mod design_precision;
pub mod design_resize;
pub mod design_styles;
pub mod design_variable_project;
pub mod design_variables;
pub mod design_vectors;
pub mod develop_edits;
pub mod diagram;
pub mod diagram_library;
pub mod distort;
pub mod document;
pub mod drawing_guides;
pub mod fragment;
pub mod geometry;
pub mod graph;
pub mod history;
pub mod layer_links;
pub mod motion;
pub mod node;
pub mod photo_source;
pub mod project;
pub mod raw;
pub mod smart;
pub mod storyboard;
pub mod storyboard_animatic;
pub mod storyboard_breakdown;
pub mod storyboard_changes;
pub mod storyboard_conform;
pub mod storyboard_estimate;
pub mod storyboard_extract;
pub mod storyboard_fingerprint;
pub mod storyboard_library;
pub mod storyboard_merge;
pub mod storyboard_motion;
pub mod storyboard_naming;
pub mod storyboard_review;
pub mod storyboard_sharing;
pub mod storyboard_shot;
pub mod storyboard_stage;
pub mod storyboard_text;
pub mod storyboard_versions;
pub mod storyboard_voices;
#[cfg(test)]
mod storyboard_voices_tests;
pub mod styles;
pub mod text;
pub mod text_effects;
pub mod timeline;
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
mod drawing_tools_tests;
#[cfg(test)]
mod strokes_layer_tests;
#[cfg(test)]
mod style_memory_tests;

mod composite_mask_cache;

pub mod design_fonts;

pub mod design_brand_assets;

pub mod design_data;

mod design_component_inference;

pub mod smart_source;

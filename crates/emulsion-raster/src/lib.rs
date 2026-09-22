//! `emulsion-raster` — tiled pixel storage, colour conversion, mip chains,
//! blend modes, per-pixel adjustments, and the CPU compositor.
//!
//! Pixels are stored as 16-bit linear-light RGBA with premultiplied alpha,
//! in sparse 256×256 tiles shared through `Arc`. All math runs in `f32`.

pub mod adjust;
pub mod blend;
pub mod color;
pub mod composite;
pub mod fill;
pub mod geom;
pub mod image;
pub mod library;
pub mod liquify;
pub mod paint;
pub mod paint_accel;
pub mod quickshape;
pub mod select;
pub mod tile;
pub mod vector;
pub mod vector_geometry;
pub mod warp;

pub use adjust::Adjustment;
pub use blend::BlendMode;
pub use composite::{CompositeNode, CompositeTree, NodeContent, Placement, render_tile};
pub use geom::{IRect, TileCoord};
pub use image::{Mask, Raster};
pub use tile::{TILE, TILE_PX};

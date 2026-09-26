//! The GPU canvas engine: an owned wgpu render loop for Emulsion documents.
//!
//! Raster layers live in a tiled `Rgba16Unorm` atlas and are composited by one
//! screen pass that runs the document as an op program; editable paths and text
//! are drawn by Vello into separate targets and blended at their stack position.
//! Blend kernels are spliced from `emulsion-gpu`, so results match the CPU
//! compositor.
//!
//! The engine does not own a window. A host hands it a device (its own, or
//! GPUI's via [`gpu::Gpu::from_shared`]) and a texture view to render into. See
//! `spikes/vello-canvas` for the standalone window and the measurements that
//! justify this design.

pub mod atlas;
pub mod brush;
pub mod cache;
pub mod canvas;
pub mod compositor;
pub mod engine;
pub mod gpu;
#[cfg(feature = "gpui")]
pub mod host;
pub mod vector;

pub use canvas::Canvas;
pub use compositor::Camera;
pub use engine::{Engine, FrameTimes, Offscreen, Output};
pub use gpu::{Gpu, TileFormat};
